//! Own encoded bytes independently of raster buffers and their alignment.
use crate::Buffer;
#[cfg(feature = "native-codecs")]
use crate::Error;
use std::ops::Deref;

enum Storage {
    Builtin(Buffer),
    #[cfg(feature = "native-codecs")]
    Native {
        frame: Vec<u8>,
        offset: usize,
    },
}

pub struct EncodedBytes(Storage);

impl EncodedBytes {
    #[cfg(feature = "native-codecs")]
    pub(super) fn from_native_frame(frame: Vec<u8>, offset: usize) -> Result<Self, Error> {
        let length = frame
            .len()
            .checked_sub(offset)
            .filter(|length| *length <= crate::MAX_FILE)
            .ok_or("native output exceeds limit")?;
        debug_assert_eq!(length + offset, frame.len());
        Ok(Self(Storage::Native { frame, offset }))
    }

    pub fn as_slice(&self) -> &[u8] {
        match &self.0 {
            Storage::Builtin(buffer) => buffer.as_slice(),
            #[cfg(feature = "native-codecs")]
            Storage::Native { frame, offset } => &frame[*offset..],
        }
    }
}

impl From<Buffer> for EncodedBytes {
    fn from(buffer: Buffer) -> Self {
        Self(Storage::Builtin(buffer))
    }
}

impl Deref for EncodedBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}
