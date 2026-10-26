#![no_std]

/// Exercise both directions and metadata with variable inputs, retaining their
/// code/data at final link. This is a footprint probe, not runnable firmware.
#[inline(never)]
pub fn convert(seconds: i64, zone_index: u32) -> i64 {
    #[cfg(not(feature = "utc"))]
    {
        core::hint::black_box((seconds, zone_index));
        0
    }
    #[cfg(feature = "utc")]
    {
        use timezone::{AVAILABLE_ZONE_IDS, LocalResolution, Zone};
        let id = AVAILABLE_ZONE_IDS[zone_index as usize % AVAILABLE_ZONE_IDS.len()];
        let Ok(zone) = Zone::get(id) else {
            return i64::MIN;
        };
        let nanos = i128::from(seconds) * 1_000_000_000;
        let Ok(local) = zone.utc_to_local(nanos) else {
            return i64::MIN;
        };
        let round_trip = match zone.local_to_utc(local.datetime()) {
            Ok(LocalResolution::Unique(value)) => value == nanos,
            Ok(LocalResolution::Ambiguous { earlier, later }) => nanos == earlier || nanos == later,
            _ => false,
        };
        if !round_trip {
            return i64::MIN;
        }
        let mut result = i64::from(local.offset_seconds());
        for byte in local.abbreviation().bytes() {
            result = result.wrapping_mul(31).wrapping_add(i64::from(byte));
        }
        result.wrapping_add(i64::from(local.datetime().year()))
    }
}
