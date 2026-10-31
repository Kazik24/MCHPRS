/// The RedstoneFun rank order. Database group weights are all equal, so they
/// cannot distinguish a player's rank from its inherited permission groups.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    #[default]
    Player,
    Builder,
    Advanced,
    Expert,
    Engineer,
    Moderator,
    Admin,
}

impl Rank {
    pub fn from_group(group: &str) -> Option<Self> {
        Some(match group {
            "default" => Self::Player,
            "builder" => Self::Builder,
            "advanced" => Self::Advanced,
            "expert" => Self::Expert,
            "engineer" => Self::Engineer,
            "moderator" => Self::Moderator,
            "admin" => Self::Admin,
            _ => return None,
        })
    }

    pub fn group(self) -> &'static str {
        match self {
            Self::Player => "default",
            Self::Builder => "builder",
            Self::Advanced => "advanced",
            Self::Expert => "expert",
            Self::Engineer => "engineer",
            Self::Moderator => "moderator",
            Self::Admin => "admin",
        }
    }

    pub fn default_prefix(self) -> &'static str {
        match self {
            Self::Admin => "&8[&4&lA&8] &c",
            Self::Moderator => "&8[&2&lM&8] &a",
            Self::Engineer => "&8[&bI&8] &b",
            Self::Expert => "&8[&5E&8] &d",
            Self::Advanced => "&8[&6Z&8] &6",
            Self::Builder => "&8[&eB&8] &e",
            Self::Player => "&8[&7G&8] &7",
        }
    }
}

#[derive(Debug)]
pub struct RankProfile {
    pub rank: Rank,
    pub prefix: Option<String>,
}
