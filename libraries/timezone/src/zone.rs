use crate::{CivilDateTime, Error, Result, ZoneRules};
use jiff::{
    Timestamp,
    tz::{AmbiguousOffset, TimeZoneOffsetInfo},
};

static UTC_RULES: ZoneRules = ZoneRules::UTC;
#[cfg(feature = "zone-europe-paris")]
static PARIS: ZoneRules = jiff::tz::get!("Europe/Paris");
#[cfg(feature = "zone-america-new-york")]
static NEW_YORK: ZoneRules = jiff::tz::get!("America/New_York");
#[cfg(feature = "zone-asia-kathmandu")]
static KATHMANDU: ZoneRules = jiff::tz::get!("Asia/Kathmandu");
#[cfg(feature = "zone-asia-kolkata")]
static KOLKATA: ZoneRules = jiff::tz::get!("Asia/Kolkata");
#[cfg(feature = "zone-australia-lord-howe")]
static LORD_HOWE: ZoneRules = jiff::tz::get!("Australia/Lord_Howe");

/// Canonical IDs available through `Zone::get`; `Etc/UTC` also aliases UTC.
/// Custom `Zone::from_static` instances do not modify this registry.
pub const AVAILABLE_ZONE_IDS: &[&str] = &[
    "UTC",
    #[cfg(feature = "zone-europe-paris")]
    "Europe/Paris",
    #[cfg(feature = "zone-america-new-york")]
    "America/New_York",
    #[cfg(feature = "zone-asia-kathmandu")]
    "Asia/Kathmandu",
    #[cfg(feature = "zone-asia-kolkata")]
    "Asia/Kolkata",
    #[cfg(feature = "zone-australia-lord-howe")]
    "Australia/Lord_Howe",
];

/// A reusable reference to immutable, compiled timezone rules. No current-zone
/// state is kept here or in the UTC clock; runtime preference selection is separate.
#[derive(Clone, Copy, Debug)]
pub struct Zone {
    id: &'static str,
    rules: &'static ZoneRules,
}

/// Local civil fields and their exact originating Unix instant.
/// Abbreviation storage is bounded and owned by the result, not allocated.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalTime {
    datetime: CivilDateTime,
    utc_nanoseconds: i128,
    zone_id: &'static str,
    info: TimeZoneOffsetInfo<'static>,
}

impl LocalTime {
    pub fn datetime(&self) -> CivilDateTime {
        self.datetime
    }
    pub fn utc_nanoseconds(&self) -> i128 {
        self.utc_nanoseconds
    }
    pub fn zone_id(&self) -> &'static str {
        self.zone_id
    }
    /// Seconds east of UTC. Historical offsets can include seconds.
    pub fn offset_seconds(&self) -> i32 {
        self.info.offset().seconds()
    }
    /// Informational abbreviation; not a unique timezone identifier or locale text.
    pub fn abbreviation(&self) -> &str {
        self.info.abbreviation()
    }
    pub fn is_dst(&self) -> bool {
        self.info.dst().is_dst()
    }
}

/// The full resolution of a civil input, with no default disambiguation policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalResolution {
    Unique(i128),
    /// Two real instants ordered by UTC. The caller must explicitly choose.
    Ambiguous {
        earlier: i128,
        later: i128,
    },
    /// No instant has these civil fields. The caller must reject or request a
    /// different input; the library does not shift it across the gap.
    Nonexistent {
        before_offset_seconds: i32,
        after_offset_seconds: i32,
    },
}

impl Zone {
    pub const UTC: Self = Self {
        id: "UTC",
        rules: &UTC_RULES,
    };

    /// Look up a case-sensitive ID in this build's selected subset. Never consults
    /// host files, environment, locale, network or a mandatory full database.
    pub fn get(id: &str) -> Result<Self> {
        match id {
            "UTC" | "Etc/UTC" => Ok(Self::UTC),
            #[cfg(feature = "zone-europe-paris")]
            "Europe/Paris" => Self::from_static(&PARIS),
            #[cfg(feature = "zone-america-new-york")]
            "America/New_York" => Self::from_static(&NEW_YORK),
            #[cfg(feature = "zone-asia-kathmandu")]
            "Asia/Kathmandu" => Self::from_static(&KATHMANDU),
            #[cfg(feature = "zone-asia-kolkata")]
            "Asia/Kolkata" => Self::from_static(&KOLKATA),
            #[cfg(feature = "zone-australia-lord-howe")]
            "Australia/Lord_Howe" => Self::from_static(&LORD_HOWE),
            _ => Err(Error::UnavailableZone),
        }
    }

    /// Reuse a caller-compiled named IANA zone without expanding the registry.
    /// For example, compile one `jiff::tz::get!("Europe/London")` static with Jiff's
    /// `static` feature, then pass its reference. Fixed/unnamed rules are rejected.
    /// The caller owns provenance/version tracking for externally supplied rules;
    /// `DATABASE_VERSION` describes this crate's selected bundled data only.
    pub fn from_static(rules: &'static ZoneRules) -> Result<Self> {
        let id = rules.iana_name().ok_or(Error::UnavailableZone)?;
        Ok(Self { id, rules })
    }

    pub fn id(self) -> &'static str {
        self.id
    }

    /// Convert signed Unix nanoseconds to civil fields in this zone. This does not
    /// validate a clock's synchronization state: callers supplying raw timestamps
    /// must establish epoch validity first (or use the optional runtime adapter).
    /// Jiff's checked timestamp range is deliberately narrower than i128.
    pub fn utc_to_local(self, utc_nanoseconds: i128) -> Result<LocalTime> {
        let timestamp =
            Timestamp::from_nanosecond(utc_nanoseconds).map_err(|_| Error::OutOfRange)?;
        let info = self.rules.to_offset_info(timestamp);
        Ok(LocalTime {
            datetime: info.offset().to_datetime(timestamp),
            utc_nanoseconds,
            zone_id: self.id,
            info,
        })
    }

    /// Resolve civil input to signed Unix nanoseconds. Gaps and overlaps are
    /// explicit values, including non-hour and historical offset changes.
    pub fn local_to_utc(self, local: CivilDateTime) -> Result<LocalResolution> {
        let candidate = self.rules.to_ambiguous_timestamp(local);
        match candidate.offset() {
            AmbiguousOffset::Unambiguous { .. } => {
                let timestamp = candidate.unambiguous().map_err(|_| Error::OutOfRange)?;
                Ok(LocalResolution::Unique(timestamp.as_nanosecond()))
            }
            AmbiguousOffset::Fold { .. } => {
                let earlier = candidate.earlier().map_err(|_| Error::OutOfRange)?;
                let later = candidate.later().map_err(|_| Error::OutOfRange)?;
                Ok(LocalResolution::Ambiguous {
                    earlier: earlier.as_nanosecond(),
                    later: later.as_nanosecond(),
                })
            }
            AmbiguousOffset::Gap { before, after } => Ok(LocalResolution::Nonexistent {
                before_offset_seconds: before.seconds(),
                after_offset_seconds: after.seconds(),
            }),
        }
    }
}
