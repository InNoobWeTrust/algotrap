/// A stream item pairing an identifier with a kernel input or output value.
pub struct Stamped<Id, T> {
    pub id: Id,
    pub value: T,
}
