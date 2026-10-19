use num::{Num, NumCast, ToPrimitive, Unsigned};

pub trait Size: Unsigned + Num + NumCast + PartialOrd + ToPrimitive + Copy {}

impl Size for u32 {}

impl Size for u64 {}

impl Size for u128 {}

impl Size for usize {}
