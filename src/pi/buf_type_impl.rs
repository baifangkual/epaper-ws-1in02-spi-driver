pub trait Bytes {
    fn bytes(&self) -> &[u8];
}

impl Bytes for u8 {
    fn bytes(&self) -> &[u8] {
        std::slice::from_ref(self)
    }
}
impl Bytes for Vec<u8> {
    fn bytes(&self) -> &[u8] {
        self.as_slice()
    }
}
impl Bytes for &[u8] {
    fn bytes(&self) -> &[u8] {
        self
    }
}
impl Bytes for &[u8; 1280] {
    fn bytes(&self) -> &[u8] {
        *self
    }
}

