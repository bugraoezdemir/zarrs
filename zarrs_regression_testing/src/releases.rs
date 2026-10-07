//! Previous `zarrs` releases tested for data compatibility.

/// A previous minor release of `zarrs` (`0.<minor>`, resolved to the latest patch release).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Release(pub(crate) u32);

/// Tested releases, newest first.
///
/// The first entry is the latest release, which is tested in CI.
/// Add new releases to the front of this list when they are published.
pub(crate) const RELEASES: &[Release] = &[
    Release(23),
    Release(22),
    Release(21),
    Release(20),
    Release(19),
    Release(18),
    Release(17),
    Release(16),
    Release(15),
    Release(14),
    Release(13),
    Release(12),
    Release(11),
    Release(10),
    Release(9),
    Release(8),
    Release(7),
    Release(6),
    Release(5),
    Release(4),
    Release(3),
    Release(2),
];

impl std::fmt::Display for Release {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "0.{}", self.0)
    }
}

impl Release {
    /// The `zarrs` features enabled in the helper for this release.
    pub(crate) fn features(self) -> Vec<&'static str> {
        let minor = self.0;
        let mut features = vec!["blosc", "gzip", "crc32c", "transpose", "zstd"];
        if (7..=10).contains(&minor) {
            // Broken feature gating in these releases
            features.extend(["async", "ndarray"]);
        }
        let since = [
            // Sharding is a feature prior to 0.16 that fails to compile with recent toolchains (`is_multiple_of` ambiguity)
            (16, "sharding"),
            (6, "bitround"),
            (6, "zfp"),
            (13, "bz2"),
            (13, "pcodec"),
            (16, "gdeflate"),
            (17, "filesystem"),
            (19, "fletcher32"),
            (20, "zlib"),
            (21, "float8"),
            (22, "adler32"),
            (23, "microfloat"),
        ];
        features.extend(
            since
                .into_iter()
                .filter(|(since, _)| minor >= *since)
                .map(|(_, feature)| feature),
        );
        features
    }

    /// The helper adapter source for this release (bridges API differences between releases).
    pub(crate) fn adapter(self) -> &'static str {
        match self.0 {
            23.. => include_str!("../helper/adapter_v0_23.rs"),
            20..=22 => include_str!("../helper/adapter_v0_20.rs"),
            17..=19 => include_str!("../helper/adapter_v0_17.rs"),
            16 => include_str!("../helper/adapter_v0_16.rs"),
            15 | 4 | 5 => include_str!("../helper/adapter_v0_4.rs"),
            11..=14 => include_str!("../helper/adapter_v0_11.rs"),
            6..=10 => include_str!("../helper/adapter_v0_6.rs"),
            _ => include_str!("../helper/adapter_v0_2.rs"),
        }
    }
}
