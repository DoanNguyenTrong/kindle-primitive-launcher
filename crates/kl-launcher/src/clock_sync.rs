use std::{
    io,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

const SERVER: &str = "time.cloudflare.com";
const TIMEOUT: Duration = Duration::from_secs(15);

pub struct ClockSync {
    child: Child,
    deadline: Instant,
}

impl ClockSync {
    pub fn start() -> io::Result<Self> {
        let mut command = Command::new(kl_kindle::device::NTPDATE);
        command.args(["-u", "-p", "2", "-t", "2", SERVER]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::process::CommandExt;
            let parent = std::process::id() as libc::pid_t;
            // SAFETY: the pre-exec hook only calls libc and uses stack data. Kill the helper
            // with Launcher even when Launcher exits directly to an app or the Kindle UI.
            unsafe {
                command.pre_exec(move || {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    if libc::getppid() != parent {
                        return Err(io::Error::from(io::ErrorKind::Interrupted));
                    }
                    Ok(())
                });
            }
        }
        Ok(Self {
            child: command.spawn()?,
            deadline: Instant::now() + TIMEOUT,
        })
    }

    /// The event loop checks this on its existing ticks. Network waits stay in the child.
    pub fn poll(&mut self) -> Option<Result<(), &'static str>> {
        match self.child.try_wait() {
            Ok(Some(status)) if status.success() => Some(Ok(())),
            Ok(Some(_)) | Err(_) => Some(Err(
                "Clock sync failed. Check your Wi-Fi connection and try again.",
            )),
            Ok(None) if Instant::now() >= self.deadline => {
                Some(Err("Clock sync timed out. Try again."))
            }
            Ok(None) => None,
        }
    }
}

impl Drop for ClockSync {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unresponsive_clock_job_times_out_without_blocking_the_event_loop() {
        let child = Command::new("sleep").arg("60").spawn().unwrap();
        let mut job = ClockSync {
            child,
            deadline: Instant::now(),
        };
        assert_eq!(job.poll(), Some(Err("Clock sync timed out. Try again.")));
    }
}
