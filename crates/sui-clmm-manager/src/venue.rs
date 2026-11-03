//! Which concentrated-liquidity / swap venue a pool belongs to.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Venue {
    #[default]
    Bluefin,
    Momentum,
    Cetus,
    Turbos,
    Deepbook,
}

impl Venue {
    pub const ALL: [Venue; 5] = [
        Venue::Bluefin,
        Venue::Momentum,
        Venue::Cetus,
        Venue::Turbos,
        Venue::Deepbook,
    ];

    pub fn brand(self) -> &'static str {
        match self {
            Self::Bluefin => "Bluefin",
            Self::Momentum => "Momentum",
            Self::Cetus => "Cetus",
            Self::Turbos => "Turbos",
            Self::Deepbook => "DeepBook",
        }
    }

    pub fn product(self) -> &'static str {
        match self {
            Self::Bluefin => "Spot",
            Self::Momentum => "CLMM",
            Self::Cetus => "CLMM",
            Self::Turbos => "CLMM",
            Self::Deepbook => "CLOB",
        }
    }

    pub fn short(self) -> &'static str {
        match self {
            Self::Bluefin => "bluefin",
            Self::Momentum => "momentum",
            Self::Cetus => "cetus",
            Self::Turbos => "turbos",
            Self::Deepbook => "deepbook",
        }
    }

    pub fn full(self) -> String {
        format!("{} {}", self.brand(), self.product())
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "bluefin" => Some(Self::Bluefin),
            "momentum" => Some(Self::Momentum),
            "cetus" => Some(Self::Cetus),
            "turbos" => Some(Self::Turbos),
            "deepbook" | "deep" => Some(Self::Deepbook),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VenueFilter {
    #[default]
    All,
    Only(Venue),
}

impl VenueFilter {
    pub fn label(self) -> String {
        match self {
            Self::All => "all venues".into(),
            Self::Only(v) => v.full(),
        }
    }

    pub fn short(self) -> String {
        match self {
            Self::All => "all".into(),
            Self::Only(v) => v.short().into(),
        }
    }

    pub fn matches(self, venue: Venue) -> bool {
        match self {
            Self::All => true,
            Self::Only(v) => v == venue,
        }
    }

    /// `all` → Bluefin → Momentum → Cetus → Turbos → DeepBook → `all`.
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Only(Venue::Bluefin),
            Self::Only(Venue::Bluefin) => Self::Only(Venue::Momentum),
            Self::Only(Venue::Momentum) => Self::Only(Venue::Cetus),
            Self::Only(Venue::Cetus) => Self::Only(Venue::Turbos),
            Self::Only(Venue::Turbos) => Self::Only(Venue::Deepbook),
            Self::Only(Venue::Deepbook) => Self::All,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_cycles() {
        let mut f = VenueFilter::All;
        for expected in [
            Venue::Bluefin,
            Venue::Momentum,
            Venue::Cetus,
            Venue::Turbos,
            Venue::Deepbook,
        ] {
            f = f.next();
            assert_eq!(f, VenueFilter::Only(expected));
        }
        f = f.next();
        assert_eq!(f, VenueFilter::All);
    }
}
