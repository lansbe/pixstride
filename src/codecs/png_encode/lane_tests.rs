use super::*;
#[test]
fn vector_filter_scores_match_the_scalar_reference_at_lane_boundaries() {
    for channels in [1, 2, 3, 4] {
        for width in [1, 3, 4, 5, 15, 16, 17, 31, 32, 33, 257, 40_001] {
            let current = (0..width * channels)
                .map(|i| (i * 73 + 19) as u8)
                .collect::<Vec<_>>();
            let previous = (0..width * channels)
                .map(|i| (i * 137 + 251) as u8)
                .collect::<Vec<_>>();
            assert_eq!(
                score_filters(&current, &previous, channels),
                score_filters_scalar(&current, &previous, channels),
                "{width} pixels, {channels} channels"
            );
            for kind in 0..5 {
                let mut actual = Vec::new();
                append_filtered(&mut actual, &current, &previous, channels, kind);
                let expected: Vec<_> = (0..current.len())
                    .map(|i| {
                        filtered(
                            kind,
                            current[i],
                            if i >= channels {
                                current[i - channels]
                            } else {
                                0
                            },
                            previous[i],
                            if i >= channels {
                                previous[i - channels]
                            } else {
                                0
                            },
                        )
                    })
                    .collect();
                assert_eq!(
                    actual, expected,
                    "filter {kind}, {width} pixels, {channels} channels"
                );
            }
        }
    }
}
