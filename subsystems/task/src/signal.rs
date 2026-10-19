use core::fmt::Debug;

/// POSIX signals enumeration
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum Signal {
    /// Hangup detected on controlling terminal or death of controlling process (SIGHUP)
    Hangup = 0,
    /// Interrupt from keyboard (SIGINT)
    Interrupt,
    /// Quit from keyboard (SIGQUIT)
    Quit,
    /// Unused : Illegal Instruction (SIGILL)
    IllegalInstruction,
    /// Trace/breakpoint trap (SIGTRAP)
    Trap,
    /// Abort signal from abort(3) (SIGABRT)
    Abort,
    /// Bus error (bad memory access) (SIGBUS)
    BusError,
    /// Floating-point exception (SIGFPE)
    FloatingPointException,
    /// Kill signal (SIGKILL)
    Kill,
    /// User-defined signal 1 (SIGUSR1)
    User1,
    /// Invalid memory reference (SIGSEGV)
    SegmentationFault,
    /// User-defined signal 2 (SIGUSR2)
    User2,
    /// Broken pipe: write to pipe with no readers (SIGPIPE)
    BrokenPipe,
    /// Timer signal from alarm(2) (SIGALRM)
    Alarm,
    /// Termination signal (SIGTERM)
    Termination,
    /// Stack fault on coprocessor (unused) (SIGSTKFLT)
    StackFault,
    /// Child stopped or terminated (SIGCHLD)
    Child,
    /// Continue if stopped (SIGCONT)
    Continue,
    /// Stop process (SIGSTOP)
    Stop,
    /// Stop typed at terminal (SIGTSTP)
    TerminalStop,
    /// Terminal input for background process (SIGTTIN)
    TerminalInput,
    /// Terminal output for background process (SIGTTOU)
    TerminalOutput,
    /// Urgent condition on socket (4.2BSD) (SIGURG)
    Urgent,
    /// CPU time limit exceeded (4.2BSD) (SIGXCPU)
    CpuTimeLimitExceeded,
    /// File size limit exceeded (4.2BSD) (SIGXFSZ)
    FileSizeLimitExceeded,
    /// Virtual alarm clock (4.2BSD) (SIGVTALRM)
    VirtualAlarm,
    /// Profiling timer expired (SIGPROF)
    ProfilingTimerExpired,
    /// Window resize signal (4.3BSD, Sun) (SIGWINCH)
    WindowResize,
    /// I/O now possible (4.2BSD) (SIGIO)
    IoPossible,
    /// Power failure (System V) (SIGPWR)
    PowerFailure,
    /// Bad system call (SVr4) (SIGSYS)
    BadSystemCall,
}

impl Signal {
    pub const FIRST: Self = Self::Hangup;
    pub const LAST: Self = Self::BadSystemCall;

    pub const fn get_discriminant(&self) -> u8 {
        *self as u8
    }
}

#[derive(Debug, Copy, Clone)]
#[repr(transparent)]
pub struct SignalAccumulator {
    accumulator: u32,
}

impl Default for SignalAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalAccumulator {
    pub const fn new() -> Self {
        Self { accumulator: 0 }
    }

    pub const fn send(&mut self, signal: Signal) {
        self.accumulator |= 1 << signal as u32;
    }

    pub const fn clear(&mut self, signal: Signal) {
        self.accumulator &= !(1 << signal as u32);
    }

    pub const fn has_signal(&self, signal: Signal) -> bool {
        self.accumulator & (1 << signal as u32) != 0
    }

    pub fn peek(&self) -> Option<Signal> {
        for bit in Signal::FIRST as u8..=Signal::LAST as u8 {
            let signal = unsafe { core::mem::transmute::<u8, Signal>(bit) };

            if self.has_signal(signal) {
                return Some(signal);
            }
        }

        None
    }

    pub fn pop(&mut self) -> Option<Signal> {
        if let Some(signal) = self.peek() {
            self.clear(signal);

            Some(signal)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_send_and_has_signal() {
        let mut acc = SignalAccumulator::new();
        acc.send(Signal::Interrupt);
        assert!(acc.has_signal(Signal::Interrupt));
    }

    #[test]
    fn test_clear_signal() {
        let mut acc = SignalAccumulator::new();
        acc.send(Signal::Quit);
        acc.clear(Signal::Quit);
        assert!(!acc.has_signal(Signal::Quit));
    }

    #[test]
    fn test_peek_and_pop() {
        let mut acc = SignalAccumulator::new();
        acc.send(Signal::Hangup);
        acc.send(Signal::User1);
        assert_eq!(acc.peek(), Some(Signal::Hangup));
        assert_eq!(acc.pop(), Some(Signal::Hangup));
        assert_eq!(acc.peek(), Some(Signal::User1));
        assert_eq!(acc.pop(), Some(Signal::User1));
        assert_eq!(acc.pop(), None);
    }

    #[test]
    fn test_signal_discriminant() {
        assert_eq!(
            Signal::PowerFailure.get_discriminant(),
            Signal::PowerFailure as u8
        );
    }
}
