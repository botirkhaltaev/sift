/// Private sorted-delta codec trait for postings and trigram-set storage.
///
/// Implementations encode sorted value lists as delta-varint using `unsigned-varint`.
/// Decode returns an error on malformed input, overflow, or unsorted values.
pub(super) trait SortedDeltaCodec {
    type Item;

    fn encode_sorted(out: &mut Vec<u8>, values: &[Self::Item]) -> std::io::Result<()>;

    fn decode_sorted(bytes: &[u8]) -> std::io::Result<Vec<Self::Item>>;
}
