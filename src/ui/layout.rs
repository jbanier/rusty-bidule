use ratatui::text::Line;

use super::count_wrapped_rows;

/// Configuration for terminal layout dimensions and constraints
struct LayoutConfig {
    // Fixed heights for static UI elements
    header_rows: u16,
    separator_rows: u16,
    activity_rows: u16,
    footer_rows: u16,

    // Adaptive constraints for flexible regions
    transcript_min_rows_normal: u16,
    transcript_min_rows_multiline: u16,
    transcript_min_ratio: f32, // Proportional minimum (e.g., 0.3 = 30% of available space)

    input_max_rows: u16,
    input_max_ratio: f32, // Proportional maximum (e.g., 0.4 = 40% of available space)
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            // Match existing hardcoded constants
            header_rows: 2,
            separator_rows: 1,
            activity_rows: 1,
            footer_rows: 1,

            transcript_min_rows_normal: 10,
            transcript_min_rows_multiline: 6,
            transcript_min_ratio: 0.3, // 30% minimum on large terminals

            input_max_rows: 10,
            input_max_ratio: 0.4, // 40% maximum on large terminals
        }
    }
}

/// Layout mode affects space allocation priorities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutMode {
    Normal,
    Multiline,
}

/// Computed dimensions for each UI region
#[derive(Debug, Clone, Copy)]
pub(crate) struct ComputedRegions {
    pub header_height: u16,
    pub separator_height: u16,
    pub transcript_min_height: u16,
    pub activity_height: u16,
    pub input_height: u16,
    pub footer_height: u16,
}

/// Manages terminal layout state and recomputation
#[derive(Debug)]
pub(crate) struct LayoutState {
    terminal_size: (u16, u16),
    mode: LayoutMode,
    pub regions: ComputedRegions,
    dirty: bool,
}

impl LayoutState {
    /// Compute layout regions based on terminal size, mode, and input content
    pub fn compute(
        terminal_width: u16,
        terminal_height: u16,
        mode: LayoutMode,
        input_lines: &[Line<'_>],
    ) -> Self {
        let config = LayoutConfig::default();

        // Calculate total fixed row requirements
        let fixed_rows = config.header_rows
            + config.separator_rows
            + config.activity_rows
            + config.footer_rows;

        // Available space for flexible regions (transcript + input)
        let available_height = terminal_height.saturating_sub(fixed_rows);

        // Compute transcript minimum using both absolute and proportional constraints
        // On tiny terminals, gracefully degrade by capping at available space
        let transcript_min_absolute = match mode {
            LayoutMode::Normal => config.transcript_min_rows_normal,
            LayoutMode::Multiline => config.transcript_min_rows_multiline,
        };
        let transcript_min_proportional =
            (available_height as f32 * config.transcript_min_ratio) as u16;
        let transcript_min_desired = transcript_min_absolute.max(transcript_min_proportional);

        // Reserve at least 1 row for input, cap transcript accordingly
        let transcript_min_height = transcript_min_desired
            .min(available_height.saturating_sub(1))
            .max(1); // Always at least 1 row

        // Compute input height with multiple constraints
        let input_desired = count_wrapped_rows(input_lines, terminal_width).min(u16::MAX as usize) as u16;
        let input_max_by_config = config.input_max_rows;
        let input_max_by_ratio = (available_height as f32 * config.input_max_ratio) as u16;
        let input_max_by_space = available_height.saturating_sub(transcript_min_height);

        let input_height = input_desired
            .min(input_max_by_config)
            .min(input_max_by_ratio)
            .min(input_max_by_space)
            .max(1); // Always at least 1 row

        Self {
            terminal_size: (terminal_width, terminal_height),
            mode,
            regions: ComputedRegions {
                header_height: config.header_rows,
                separator_height: config.separator_rows,
                transcript_min_height,
                activity_height: config.activity_rows,
                input_height,
                footer_height: config.footer_rows,
            },
            dirty: false,
        }
    }

    /// Check if layout needs recomputation
    pub fn needs_refresh(
        &self,
        terminal_width: u16,
        terminal_height: u16,
        mode: LayoutMode,
    ) -> bool {
        self.terminal_size != (terminal_width, terminal_height) || self.mode != mode || self.dirty
    }

    /// Mark layout as needing recomputation
    pub fn invalidate(&mut self) {
        self.dirty = true;
    }
}

impl Default for LayoutState {
    fn default() -> Self {
        // Initialize with reasonable defaults for 80x24 terminal
        Self::compute(80, 24, LayoutMode::Normal, &[])
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;

    #[test]
    fn layout_adapts_to_tiny_terminal() {
        let layout = LayoutState::compute(40, 12, LayoutMode::Normal, &[]);

        // Fixed rows: 2 + 1 + 1 + 1 = 5
        // Available: 12 - 5 = 7 rows for transcript + input
        // Should allocate at least 1 row to each region
        assert!(layout.regions.header_height == 2);
        assert!(layout.regions.transcript_min_height >= 1);
        assert!(layout.regions.input_height >= 1);
        assert!(layout.regions.footer_height == 1);

        // Total should not exceed terminal height
        let total = layout.regions.header_height
            + layout.regions.separator_height
            + layout.regions.transcript_min_height
            + layout.regions.activity_height
            + layout.regions.input_height
            + layout.regions.footer_height;
        assert!(total <= 12);
    }

    #[test]
    fn layout_respects_multiline_mode() {
        let normal = LayoutState::compute(80, 30, LayoutMode::Normal, &[]);
        let multiline = LayoutState::compute(80, 30, LayoutMode::Multiline, &[]);

        // Normal mode should have larger transcript minimum than multiline
        assert!(
            normal.regions.transcript_min_height > multiline.regions.transcript_min_height,
            "normal={} should exceed multiline={}",
            normal.regions.transcript_min_height,
            multiline.regions.transcript_min_height
        );
    }

    #[test]
    fn layout_uses_proportional_on_large_terminal() {
        let layout = LayoutState::compute(120, 60, LayoutMode::Normal, &[]);

        // Fixed rows: 2 + 1 + 1 + 1 = 5
        // Available: 60 - 5 = 55 rows
        // Proportional minimum: 55 * 0.3 = 16.5 → 16 rows
        // Should exceed absolute minimum of 10 rows
        assert!(
            layout.regions.transcript_min_height >= 16,
            "expected >= 16, got {}",
            layout.regions.transcript_min_height
        );
    }

    #[test]
    fn layout_caps_input_by_ratio() {
        // Create many input lines to test ratio capping
        let many_lines: Vec<Line> = (0..50).map(|i| Line::raw(format!("line {}", i))).collect();

        let layout = LayoutState::compute(80, 60, LayoutMode::Normal, &many_lines);

        // Fixed rows: 5, Available: 55
        // Input max by ratio: 55 * 0.4 = 22 rows
        // Should not exceed 22 even though desired is 50
        assert!(
            layout.regions.input_height <= 22,
            "expected <= 22, got {}",
            layout.regions.input_height
        );

        // Should also respect the config max of 10 rows
        assert!(
            layout.regions.input_height <= 10,
            "expected <= 10, got {}",
            layout.regions.input_height
        );
    }

    #[test]
    fn layout_never_allocates_zero_rows() {
        // Edge case: extremely tiny terminal
        let layout = LayoutState::compute(10, 8, LayoutMode::Normal, &[]);

        // Every region should get at least 1 row
        assert!(layout.regions.header_height >= 1);
        assert!(layout.regions.separator_height >= 1);
        assert!(layout.regions.transcript_min_height >= 1);
        assert!(layout.regions.activity_height >= 1);
        assert!(layout.regions.input_height >= 1);
        assert!(layout.regions.footer_height >= 1);
    }

    #[test]
    fn layout_needs_refresh_detects_size_change() {
        let layout = LayoutState::compute(80, 24, LayoutMode::Normal, &[]);

        assert!(!layout.needs_refresh(80, 24, LayoutMode::Normal));
        assert!(layout.needs_refresh(100, 24, LayoutMode::Normal)); // Width changed
        assert!(layout.needs_refresh(80, 30, LayoutMode::Normal)); // Height changed
    }

    #[test]
    fn layout_needs_refresh_detects_mode_change() {
        let layout = LayoutState::compute(80, 24, LayoutMode::Normal, &[]);

        assert!(!layout.needs_refresh(80, 24, LayoutMode::Normal));
        assert!(layout.needs_refresh(80, 24, LayoutMode::Multiline)); // Mode changed
    }

    #[test]
    fn layout_invalidate_marks_dirty() {
        let mut layout = LayoutState::compute(80, 24, LayoutMode::Normal, &[]);

        assert!(!layout.needs_refresh(80, 24, LayoutMode::Normal));

        layout.invalidate();
        assert!(layout.needs_refresh(80, 24, LayoutMode::Normal));
    }
}
