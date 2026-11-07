//! Keep delayed UI timers from becoming a busy render loop.
pub fn needs_immediate_repaint(ctx: &egui::Context) -> bool {
    ctx.requested_repaint_last_pass()
}

#[cfg(test)]
mod tests {
    #[test]
    fn future_timer_does_not_request_an_immediate_frame() {
        let ctx = egui::Context::default();
        for i in 0..5 {
            let _ = ctx.run_ui(
                egui::RawInput {
                    time: Some(i as f64 * 0.01),
                    ..Default::default()
                },
                |ui| {
                    ui.label("Idle browser");
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_secs(3));
                },
            );
        }
        assert!(ctx.has_requested_repaint(), "a future timer is pending");
        assert!(
            !super::needs_immediate_repaint(&ctx),
            "a future timer must not spin the event loop"
        );
    }
}
