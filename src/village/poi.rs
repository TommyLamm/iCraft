#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum VillagerProfession {
    Unemployed,
    Farmer,
    Librarian,
    Armorer,
    Cleric,
}

impl VillagerProfession {
    pub const fn to_wire(self) -> u8 {
        match self {
            Self::Unemployed => 0,
            Self::Farmer => 1,
            Self::Librarian => 2,
            Self::Armorer => 3,
            Self::Cleric => 4,
        }
    }

    pub const fn from_wire(val: u8) -> Self {
        match val {
            1 => Self::Farmer,
            2 => Self::Librarian,
            3 => Self::Armorer,
            4 => Self::Cleric,
            _ => Self::Unemployed,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Unemployed => "Unemployed",
            Self::Farmer => "Farmer",
            Self::Librarian => "Librarian",
            Self::Armorer => "Armorer",
            Self::Cleric => "Cleric",
        }
    }
}
