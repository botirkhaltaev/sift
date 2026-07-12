use crate::search::{InputIdentity, Inputs};

pub struct ByteInput<'a> {
    pub path: std::borrow::Cow<'a, str>,
    pub bytes: std::borrow::Cow<'a, [u8]>,
    pub explicit: bool,
}

impl<'a> Inputs<'a> {
    #[must_use]
    pub fn empty() -> Self {
        Self::with_capacity(0)
    }

    #[must_use]
    pub fn with_stream(mut self, stream: ByteInput<'a>) -> Self {
        let name = stream.path.as_ref().to_string();
        if stream.explicit {
            self.push_explicit_bytes(stream.path, stream.bytes, InputIdentity::from_name(&name));
        } else {
            self.push_bytes(stream.path, stream.bytes, InputIdentity::from_name(&name));
        }
        self
    }
}
