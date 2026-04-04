extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

use crate::{mouse, wm};

// ---------------------------------------------------------------------------
// Context switch (System V AMD64 ABI: old=rdi, new=rsi)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug)]
struct TaskContext {
    rsp: u64,
}

unsafe extern "C" {
    fn switch_context(old: *mut TaskContext, new: *const TaskContext);
}

global_asm!(
    ".global switch_context",
    "switch_context:",
    "    push rbp",
    "    push rbx",
    "    push r12",
    "    push r13",
    "    push r14",
    "    push r15",
    "    mov [rdi], rsp", // save current RSP to old context
    "    mov rsp, [rsi]", // load new RSP from new context
    "    pop r15",
    "    pop r14",
    "    pop r13",
    "    pop r12",
    "    pop rbx",
    "    pop rbp",
    "    ret", // jump to return address on new stack
);

// ---------------------------------------------------------------------------
// Task
// ---------------------------------------------------------------------------

const TASK_STACK_SIZE: usize = 64 * 1024; // 64 KiB per task

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TaskState {
    Ready,
    Running,
    Sleeping,
}

struct Task {
    id: usize,
    context: TaskContext,
    #[allow(dead_code)]
    stack: Vec<u8>, // must keep alive so the stack memory isn't freed
    state: TaskState,
    window_id: Option<usize>,
}

// ---------------------------------------------------------------------------
// Scheduler
// ---------------------------------------------------------------------------

struct Scheduler {
    tasks: Vec<Task>,
    current: usize,
    next_id: usize,
}

static SCHEDULER: Mutex<Option<Scheduler>> = Mutex::new(None);
static SCHEDULER_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn init() {
    *SCHEDULER.lock() = Some(Scheduler {
        tasks: Vec::new(),
        current: 0,
        next_id: 0,
    });
}

/// Spawn a new task. `entry` is a function `fn() -> !` that never returns.
/// `window_id` is the WM window this task owns (if any).
pub fn spawn(entry: fn() -> !, window_id: Option<usize>) -> usize {
    let mut guard = SCHEDULER.lock();
    let sched = guard.as_mut().expect("scheduler not initialized");

    let id = sched.next_id;
    sched.next_id += 1;

    // Allocate stack
    let mut stack = vec![0u8; TASK_STACK_SIZE];

    // Set up initial stack frame for switch_context:
    // The stack should look like switch_context just pushed regs:
    //   [entry_point]  ← return address for `ret`
    //   [rbp=0]
    //   [rbx=0]
    //   [r12=0]
    //   [r13=0]
    //   [r14=0]
    //   [r15=0]        ← RSP points here
    let stack_top = stack.as_mut_ptr() as usize + TASK_STACK_SIZE;
    // Align to 16 bytes
    let stack_top = stack_top & !0xF;

    let sp = stack_top as *mut u64;
    unsafe {
        // Push return address (entry point)
        let sp = sp.sub(1);
        *sp = entry as u64;
        // Push 6 callee-saved registers (rbp, rbx, r12, r13, r14, r15)
        let sp = sp.sub(1);
        *sp = 0; // rbp
        let sp = sp.sub(1);
        *sp = 0; // rbx
        let sp = sp.sub(1);
        *sp = 0; // r12
        let sp = sp.sub(1);
        *sp = 0; // r13
        let sp = sp.sub(1);
        *sp = 0; // r14
        let sp = sp.sub(1);
        *sp = 0; // r15

        let task = Task {
            id,
            context: TaskContext { rsp: sp as u64 },
            stack,
            state: TaskState::Ready,
            window_id,
        };
        sched.tasks.push(task);
    }

    id
}

/// Get the window ID of the currently running task.
pub fn current_window_id() -> Option<usize> {
    x86_64::instructions::interrupts::without_interrupts(|| {
        if let Some(sched) = SCHEDULER.lock().as_ref() {
            if sched.current < sched.tasks.len() {
                return sched.tasks[sched.current].window_id;
            }
        }
        None
    })
}

/// Yield execution to the next ready task.
pub fn yield_now() {
    if !SCHEDULER_RUNNING.load(Ordering::Relaxed) {
        return;
    }

    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut guard = SCHEDULER.lock();
        let sched = match guard.as_mut() {
            Some(s) => s,
            None => return,
        };

        let count = sched.tasks.len();
        if count <= 1 {
            return;
        }

        let old_idx = sched.current;

        // Find next ready task (round-robin)
        let mut next_idx = (old_idx + 1) % count;
        let mut found = false;
        for _ in 0..count {
            if sched.tasks[next_idx].state == TaskState::Ready
                || sched.tasks[next_idx].state == TaskState::Running
            {
                found = true;
                break;
            }
            next_idx = (next_idx + 1) % count;
        }

        if !found || next_idx == old_idx {
            return;
        }

        sched.tasks[old_idx].state = TaskState::Ready;
        sched.tasks[next_idx].state = TaskState::Running;
        sched.current = next_idx;

        // Get raw pointers to contexts before dropping the lock
        let old_ctx = &mut sched.tasks[old_idx].context as *mut TaskContext;
        let new_ctx = &sched.tasks[next_idx].context as *const TaskContext;

        // Drop the lock before switching
        drop(guard);

        unsafe {
            switch_context(old_ctx, new_ctx);
        }
    });
}

/// Main scheduler loop. Called from kernel_main after spawning tasks.
/// Registers itself as the idle task (task index 0), then starts switching.
pub fn run() -> ! {
    // Register the current execution context as the idle task
    {
        let mut guard = SCHEDULER.lock();
        let sched = guard.as_mut().expect("scheduler not initialized");

        let id = sched.next_id;
        sched.next_id += 1;

        // The idle task uses the current kernel stack, so we don't allocate a stack.
        // Its context.rsp will be saved by switch_context when it yields.
        let task = Task {
            id,
            context: TaskContext { rsp: 0 },
            stack: Vec::new(), // no separate stack — uses kernel_main's stack
            state: TaskState::Running,
            window_id: None,
        };

        // Insert as the first task (index 0)
        sched.tasks.insert(0, task);
        sched.current = 0;
    }

    SCHEDULER_RUNNING.store(true, Ordering::Relaxed);

    loop {
        // Process mouse events (WM handles focus)
        while let Some(event) = mouse::try_read_event() {
            wm::handle_mouse(event);
        }

        // Composite if needed
        if wm::with_wm(|wm| wm.needs_composite()).unwrap_or(false) {
            wm::composite();
        }

        // Yield to the next user task
        yield_now();

        // If no tasks ran, halt until next interrupt
        x86_64::instructions::hlt();
    }
}
