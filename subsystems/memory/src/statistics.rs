#[derive(Debug, Clone)]
pub struct RegionStatistics {
    /// Total usable size of the heap region in bytes.
    pub size: usize,

    /// Currently used size of the heap region in bytes.
    pub used: usize,

    /// Free size of the heap region in bytes.
    pub free: usize,
}

#[derive(Debug)]
pub struct Statistics {
    /// Granular stats for all the configured memory regions.
    pub region_stats: [Option<RegionStatistics>; 3],

    /// Total size of all combined heap regions in bytes.
    pub size: usize,

    /// Current usage of the heap across all configured regions in bytes.
    pub current_usage: usize,

    /// Estimation of the max used heap in bytes.
    #[cfg(feature = "Debug")]
    pub max_usage: usize,

    /// Estimation of the total allocated bytes since initialization.
    #[cfg(feature = "Debug")]
    pub total_allocated: usize,

    /// Estimation of the total freed bytes since initialization.
    #[cfg(feature = "Debug")]
    pub total_freed: usize,
}
