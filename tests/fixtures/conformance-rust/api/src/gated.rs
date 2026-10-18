// Conditional compilation: the default configuration of an x86_64 Linux target.
pub fn always() {}

#[cfg(windows)]
pub fn windows_only() {}

#[cfg(feature = "metrics")]
pub fn metrics_only() {}

// `net` is a default feature, and `wire` is reached through it.
#[cfg(feature = "wire")]
pub fn through_net() {}

#[cfg(all(unix, not(feature = "metrics")))]
pub fn unix_without_metrics() {}

#[cfg(any(windows, target_os = "linux"))]
pub fn on_linux() {}

#[cfg(not(test))]
pub fn outside_tests() {}

// `cfg_attr` adds an attribute and leaves the item in.
#[cfg_attr(windows, derive(Debug))]
pub struct Gauge;

#[cfg(windows)]
impl Gauge {
    pub fn windows_method() {}
}

impl Gauge {
    pub fn plain() {}

    #[cfg(feature = "metrics")]
    pub fn sampled() {}

    #[cfg(unix)]
    pub fn unix_method() {}
}

#[cfg(windows)]
use std::os::windows::io::RawHandle;

#[cfg(test)]
mod tests {
    fn helper() {
        super::always();
    }
}
