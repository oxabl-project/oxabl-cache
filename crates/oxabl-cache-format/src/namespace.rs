use crate::error::{FormatError, Result};

/// Cache namespace of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namespace {
    Trusted,
    Personal,
}

impl Namespace {
    pub fn to_u8(self) -> u8 {
        match self {
            Self::Trusted => 0,
            Self::Personal => 1,
        }
    }

    pub fn from_u8(v: u8) -> Result<Self> {
        match v {
            0 => Ok(Self::Trusted),
            1 => Ok(Self::Personal),
            _ => Err(FormatError::Invalid("namespace")),
        }
    }
}

/// Set of namespaces a lookup asks about. At least one is set in an encodable value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Namespaces {
    pub trusted: bool,
    pub personal: bool,
}

impl Namespaces {
    pub fn contains(&self, ns: Namespace) -> bool {
        match ns {
            Namespace::Trusted => self.trusted,
            Namespace::Personal => self.personal,
        }
    }

    pub fn len(&self) -> usize {
        usize::from(self.trusted) + usize::from(self.personal)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn to_bits(self) -> u8 {
        u8::from(self.trusted) | u8::from(self.personal) << 1
    }

    pub(crate) fn from_bits(bits: u8) -> Result<Self> {
        if bits & !0b11 != 0 || bits == 0 {
            return Err(FormatError::Invalid("namespaces"));
        }
        Ok(Self {
            trusted: bits & 1 != 0,
            personal: bits & 2 != 0,
        })
    }
}
