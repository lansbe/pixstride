use super::*;
impl StreamFiltered {
    pub(crate) fn new(
        source_width: usize,
        source_height: usize,
        width: usize,
        height: usize,
        filter: Filter,
    ) -> Result<Option<Self>, Error> {
        // Vertical coefficients are also polyphase for the exact x2/x4
        // cases. Keep the original non-cached path as an allocation and
        // geometry fallback. This remains private to this request.
        let vertical_plan = match Plan::new_compact(source_height, height, 0, height, filter, false)
        {
            Ok(Some(plan)) => plan,
            _ => {
                let mut spans = Buffer::new(
                    height
                        .checked_mul(size_of::<Span>())
                        .ok_or("resize plan too large")?,
                )?;
                for (y, entry) in typed_mut::<Span>(&mut spans).iter_mut().enumerate() {
                    *entry = make_span(source_height, height, y, filter);
                }
                Plan {
                    spans,
                    weights: None,
                    filter,
                    opaque_weights: false,
                }
            }
        };
        let vertical = typed::<Span>(&vertical_plan.spans);
        let mut max_span = 0;
        for entry in vertical.iter() {
            max_span = max_span.max(entry.count);
        }
        // Count simultaneously active output rows using the exact same
        // clamped spans as the existing resize. Their starts and ends are
        // monotone, so each source row contributes to this interval.
        let mut started = 0;
        let mut completed = 0;
        let mut max_active = 0usize;
        for sy in 0..source_height {
            while started < height && vertical[started].first <= sy {
                started += 1;
            }
            while completed < started && vertical[completed].first + vertical[completed].count <= sy
            {
                completed += 1;
            }
            max_active = max_active.max(started - completed);
        }
        let accumulate = max_active < max_span;
        let slots = if accumulate { max_active } else { max_span };
        let row_values = width.checked_mul(CHANNELS).ok_or("resize row too large")?;
        let row_bytes = row_values
            .checked_mul(size_of::<f32>())
            .ok_or("resize row too large")?;
        let storage_bytes = slots
            .checked_mul(row_bytes)
            .ok_or("resize cache too large")?;
        if slots == 0 || storage_bytes > ROW_CACHE_BYTES {
            return Ok(None);
        }
        let stripes = width.div_ceil(STRIPE);
        if stripes > STREAM_MAX_STRIPES {
            return Ok(None);
        }
        let phase_stride = if width.checked_mul(2) == Some(source_width) {
            2
        } else if width.checked_mul(4) == Some(source_width) {
            4
        } else {
            0
        };
        let compact = cfg!(all(target_arch = "aarch64", target_endian = "little"))
            && phase_stride != 0
            && source_width >= 512
            && width >= 128;
        let mut plans: [Option<Plan>; STREAM_MAX_STRIPES] = core::array::from_fn(|_| None);
        for (index, slot) in plans[..stripes].iter_mut().enumerate() {
            let first = index * STRIPE;
            let count = (width - first).min(STRIPE);
            *slot = if compact {
                Plan::new_compact(source_width, width, first, count, filter, true)?
            } else {
                None
            };
            if slot.is_none() {
                *slot = Some(Plan::new(source_width, width, first, count, filter, true)?);
            }
        }
        let eligible = cfg!(all(target_arch = "aarch64", target_endian = "little"))
            && phase_stride != 0
            && source_width >= 512
            && width >= 128
            && plans[..stripes]
                .iter()
                .all(|plan| plan.as_ref().unwrap().opaque_weights);
        let planes = if eligible {
            Buffer::new(source_width * 3 * size_of::<f32>()).ok()
        } else {
            None
        };
        if planes.is_some() {
            for plan in &mut plans[..stripes] {
                plan.as_mut().unwrap().mark_eight(phase_stride);
            }
        }
        let opaque_alpha = if planes.is_some() {
            StreamOpaqueAlpha::from_plans(&plans[..stripes], width)
        } else {
            None
        };
        let row_values = width * if opaque_alpha.is_some() { 3 } else { CHANNELS };
        let row_bytes = row_values * size_of::<f32>();
        let storage_bytes = slots * row_bytes;
        let rgba_row = if planes.is_none() {
            Some(Buffer::new(source_width * CHANNELS)?)
        } else {
            None
        };
        Ok(Some(Self {
            vertical: vertical_plan,
            plans,
            stripes,
            rgba_row,
            planes,
            opaque_alpha,
            phase_stride,
            horizontal_row: if accumulate {
                Some(Buffer::new(row_bytes)?)
            } else {
                None
            },
            rows: Buffer::new(storage_bytes)?,
            slots,
            accumulate,
            started: 0,
        }))
    }

    pub(crate) fn write_stripe<T: BmpTarget>(
        sums: &[f32],
        bmp: &mut T,
        bmp_stride: usize,
        height: usize,
        y: usize,
        first: usize,
        pack: fn(&[u8], &mut [u8]),
    ) {
        let mut pixels = [0u8; STRIPE * CHANNELS];
        rgba_from_sums(sums, &mut pixels[..sums.len()]);
        bmp.write_rgba(bmp_stride, height, y, first, &pixels[..sums.len()], pack);
    }

    pub(crate) fn write_stripe_three<T: BmpTarget>(
        sums: &[f32],
        alpha: &StreamOpaqueAlpha,
        alpha_sums: &[f32],
        bmp: &mut T,
        bmp_stride: usize,
        height: usize,
        y: usize,
        first: usize,
        pack: fn(&[u8], &mut [u8]),
    ) {
        let count = sums.len() / 3;
        let mut pixels = [0u8; STRIPE * CHANNELS];
        rgba_from_three_sums(
            sums,
            &alpha.map.as_slice()[first..first + count],
            alpha_sums,
            &mut pixels[..count * CHANNELS],
        );
        bmp.write_rgba(
            bmp_stride,
            height,
            y,
            first,
            &pixels[..count * CHANNELS],
            pack,
        );
    }

    pub(crate) fn push_row<T: BmpTarget>(
        &mut self,
        sy: usize,
        rgb: &[u8],
        source_width: usize,
        width: usize,
        height: usize,
        next_output: &mut usize,
        bmp: &mut T,
        bmp_stride: usize,
        expand: fn(&[u8], &mut [u8]),
        pack: fn(&[u8], &mut [u8]),
    ) -> Result<(), Error> {
        if let Some(_planes) = &mut self.planes {
            #[cfg(all(target_arch = "aarch64", target_endian = "little"))]
            // SAFETY: the RGB row and three f32 phase planes have the same
            // source width; exact x2/x4 ratios make the phases equal length.
            unsafe {
                rgb_to_phase_planes(rgb, typed_mut::<f32>(_planes), self.phase_stride);
            }
            #[cfg(not(all(target_arch = "aarch64", target_endian = "little")))]
            unreachable!("phase path only exists on ARM64");
        } else {
            expand(rgb, self.rgba_row.as_mut().unwrap().as_mut_slice());
        }
        let channels = if self.opaque_alpha.is_some() {
            3
        } else {
            CHANNELS
        };
        let row_values = width * channels;
        if !self.accumulate {
            let slot = sy % self.slots;
            let target =
                &mut typed_mut::<f32>(&mut self.rows)[slot * row_values..(slot + 1) * row_values];
            for index in 0..self.stripes {
                let first = index * STRIPE;
                let count = (width - first).min(STRIPE);
                let start = first * channels;
                if channels == 3 {
                    horizontal_stream_three(
                        &self.planes,
                        source_width,
                        self.phase_stride,
                        self.plans[index].as_ref().unwrap(),
                        &mut target[start..start + count * channels],
                    );
                } else {
                    horizontal_stream(
                        &self.planes,
                        &self.rgba_row,
                        source_width,
                        self.phase_stride,
                        self.plans[index].as_ref().unwrap(),
                        &mut target[start..start + count * channels],
                    );
                }
            }
            let mut sums = [0.0f32; STRIPE * CHANNELS];
            while *next_output < height {
                let vertical = typed::<Span>(&self.vertical.spans)[*next_output];
                if vertical.first + vertical.count - 1 > sy {
                    break;
                }
                let alpha_sums = self
                    .opaque_alpha
                    .as_ref()
                    .map(|alpha| alpha.sums(&self.vertical, &vertical));
                for index in 0..self.stripes {
                    let first = index * STRIPE;
                    let values = (width - first).min(STRIPE) * channels;
                    sums[..values].fill(0.0);
                    for n in 0..vertical.count {
                        let source = vertical.first + n;
                        debug_assert!(source <= sy && sy - source < self.slots);
                        let start = (source % self.slots) * row_values + first * channels;
                        let row = &typed::<f32>(&self.rows)[start..start + values];
                        let weight = self.vertical.weight(&vertical, n);
                        for i in 0..values {
                            sums[i] += row[i] * weight;
                        }
                    }
                    if let (Some(alpha), Some(alpha_sums)) = (&self.opaque_alpha, &alpha_sums) {
                        Self::write_stripe_three(
                            &sums[..values],
                            alpha,
                            alpha_sums,
                            bmp,
                            bmp_stride,
                            height,
                            *next_output,
                            first,
                            pack,
                        );
                    } else {
                        Self::write_stripe(
                            &sums[..values],
                            bmp,
                            bmp_stride,
                            height,
                            *next_output,
                            first,
                            pack,
                        );
                    }
                }
                bmp.end_row(bmp_stride, height, *next_output)?;
                *next_output += 1;
            }
            return Ok(());
        }

        while self.started < height && typed::<Span>(&self.vertical.spans)[self.started].first <= sy
        {
            debug_assert!(self.started - *next_output < self.slots);
            let slot = self.started % self.slots;
            typed_mut::<f32>(&mut self.rows)[slot * row_values..(slot + 1) * row_values].fill(0.0);
            self.started += 1;
        }
        let row = typed_mut::<f32>(self.horizontal_row.as_mut().unwrap());
        for index in 0..self.stripes {
            let first = index * STRIPE;
            let count = (width - first).min(STRIPE);
            let start = first * channels;
            if channels == 3 {
                horizontal_stream_three(
                    &self.planes,
                    source_width,
                    self.phase_stride,
                    self.plans[index].as_ref().unwrap(),
                    &mut row[start..start + count * channels],
                );
            } else {
                horizontal_stream(
                    &self.planes,
                    &self.rgba_row,
                    source_width,
                    self.phase_stride,
                    self.plans[index].as_ref().unwrap(),
                    &mut row[start..start + count * channels],
                );
            }
        }
        for y in *next_output..self.started {
            let vertical = typed::<Span>(&self.vertical.spans)[y];
            if sy < vertical.first || sy - vertical.first >= vertical.count {
                continue;
            }
            let weight = self.vertical.weight(&vertical, sy - vertical.first);
            let slot = y % self.slots;
            let sums =
                &mut typed_mut::<f32>(&mut self.rows)[slot * row_values..(slot + 1) * row_values];
            accumulate_weighted(sums, row, weight);
        }
        while *next_output < self.started {
            let vertical = typed::<Span>(&self.vertical.spans)[*next_output];
            if vertical.first + vertical.count - 1 > sy {
                break;
            }
            let slot = *next_output % self.slots;
            let sums = &typed::<f32>(&self.rows)[slot * row_values..(slot + 1) * row_values];
            let alpha_sums = self
                .opaque_alpha
                .as_ref()
                .map(|alpha| alpha.sums(&self.vertical, &vertical));
            for index in 0..self.stripes {
                let first = index * STRIPE;
                let count = (width - first).min(STRIPE);
                let start = first * channels;
                if let (Some(alpha), Some(alpha_sums)) = (&self.opaque_alpha, &alpha_sums) {
                    Self::write_stripe_three(
                        &sums[start..start + count * channels],
                        alpha,
                        alpha_sums,
                        bmp,
                        bmp_stride,
                        height,
                        *next_output,
                        first,
                        pack,
                    );
                } else {
                    Self::write_stripe(
                        &sums[start..start + count * channels],
                        bmp,
                        bmp_stride,
                        height,
                        *next_output,
                        first,
                        pack,
                    );
                }
            }
            bmp.end_row(bmp_stride, height, *next_output)?;
            *next_output += 1;
        }
        Ok(())
    }
}
