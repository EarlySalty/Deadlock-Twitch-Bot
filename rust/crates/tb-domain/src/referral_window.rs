#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClaimWindowDuration {
    seconds: i64,
    sql_interval: &'static str,
}

impl ClaimWindowDuration {
    pub const fn seconds(self) -> i64 {
        self.seconds
    }

    pub const fn sql_interval(self) -> &'static str {
        self.sql_interval
    }
}

pub const RESERVATION_TTL: ClaimWindowDuration = ClaimWindowDuration {
    seconds: 4 * 24 * 60 * 60,
    sql_interval: "4 days",
};

pub const POST_ACTIVATION_GRACE: ClaimWindowDuration = ClaimWindowDuration {
    seconds: 24 * 60 * 60,
    sql_interval: "24 hours",
};
