//! Native global media controls fed by Servo's MediaSession events, without polling JS.
mod state;
pub(crate) use state::MediaControls;
use state::{Playback, Update};

use servo::{
    LoadStatus, MediaSessionActionType, MediaSessionEvent, MediaSessionPlaybackState, WebView,
    WebViewId,
};

use crate::window::ServoShellWindow;

pub(crate) fn received(window: &ServoShellWindow, view: &WebView, event: &MediaSessionEvent) {
    if view.load_status() == LoadStatus::Started || window.has_restoring_target(view.id()) {
        return;
    }
    let update = match event {
        MediaSessionEvent::SetMetadata(metadata) => Update::Metadata {
            title: metadata.title.clone(),
            artist: metadata.artist.clone(),
            album: metadata.album.clone(),
        },
        MediaSessionEvent::PlaybackStateChange(MediaSessionPlaybackState::Playing) => {
            Update::Playback(Playback::Playing)
        },
        MediaSessionEvent::PlaybackStateChange(MediaSessionPlaybackState::Paused) => {
            Update::Playback(Playback::Paused)
        },
        MediaSessionEvent::PlaybackStateChange(MediaSessionPlaybackState::None_) => Update::Ended,
        MediaSessionEvent::SetPositionState(position) => Update::Position {
            position: position.position,
            duration: position.duration,
        },
    };
    let mut controls = window.media_controls.borrow_mut();
    controls.document_changed(view.id(), view.document_generation());
    controls.update(view.id(), view.document_generation(), update);
    drop(controls);
    window.set_needs_update();
    window.set_needs_repaint();
}

fn dispatch(
    window: &ServoShellWindow,
    id: WebViewId,
    document: u64,
    action: MediaSessionActionType,
) {
    let Some(view) = window.webview_by_id(id) else {
        return;
    };
    if view.document_generation() != document
        || view.load_status() == LoadStatus::Started
        || window
            .media_controls
            .borrow()
            .session(id, document)
            .is_none()
    {
        return;
    }
    // Playback indicators change only after the page/engine confirms its new state.
    view.notify_media_session_action_event(action);
}

fn timestamp(seconds: f64) -> String {
    let seconds = seconds as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

pub(crate) fn button(ui: &mut egui::Ui, window: &ServoShellWindow) {
    let sessions = {
        let controls = window.media_controls.borrow();
        window
            .webviews()
            .into_iter()
            .filter_map(|(id, view)| {
                if view.load_status() == LoadStatus::Started || window.has_restoring_target(id) {
                    return None;
                }
                controls
                    .session(id, view.document_generation())
                    .cloned()
                    .map(|session| (view, session))
            })
            .collect::<Vec<_>>()
    };
    let playing = sessions
        .iter()
        .any(|(_, session)| session.playback == Playback::Playing);
    let response = ui
        .menu_button("    ", |ui| {
            ui.set_width(360.0);
            ui.strong("Медиа из вкладок");
            if sessions.is_empty() {
                ui.label("Сейчас нет активных медиасеансов.");
                return;
            }
            egui::ScrollArea::vertical()
                .max_height(440.0)
                .show(ui, |ui| {
                    for (view, session) in &sessions {
                        ui.separator();
                        let title = if session.title.is_empty() {
                            view.page_title()
                                .filter(|title| !title.is_empty())
                                .unwrap_or_else(|| "Медиа во вкладке".into())
                        } else {
                            session.title.clone()
                        };
                        ui.add(egui::Label::new(egui::RichText::new(title).strong()).truncate());
                        let subtitle = [&session.artist, &session.album]
                            .into_iter()
                            .filter(|value| !value.is_empty())
                            .map(String::as_str)
                            .collect::<Vec<_>>()
                            .join(" · ");
                        if !subtitle.is_empty() {
                            ui.add(egui::Label::new(subtitle).truncate());
                        }
                        if let Some(host) =
                            view.url().and_then(|url| url.host_str().map(str::to_owned))
                        {
                            ui.weak(host);
                        }
                        ui.horizontal(|ui| {
                            ui.weak(match session.playback {
                                Playback::Playing => "Воспроизводится",
                                Playback::Paused => "На паузе",
                                Playback::Unknown => "Медиасеанс",
                            });
                            if let Some((position, duration)) = session.position {
                                ui.weak(format!(
                                    "{} / {}",
                                    timestamp(position),
                                    timestamp(duration)
                                ));
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui
                                .button("Предыдущий")
                                .on_hover_text(
                                    "Предыдущий трек, если сайт поддерживает переключение",
                                )
                                .clicked()
                            {
                                dispatch(
                                    window,
                                    view.id(),
                                    session.document,
                                    MediaSessionActionType::PreviousTrack,
                                );
                            }
                            let (label, action) = if session.playback == Playback::Playing {
                                ("Пауза", MediaSessionActionType::Pause)
                            } else {
                                ("Играть", MediaSessionActionType::Play)
                            };
                            if ui.button(label).clicked() {
                                dispatch(window, view.id(), session.document, action);
                            }
                            if ui
                                .button("Следующий")
                                .on_hover_text(
                                    "Следующий трек, если сайт поддерживает переключение",
                                )
                                .clicked()
                            {
                                dispatch(
                                    window,
                                    view.id(),
                                    session.document,
                                    MediaSessionActionType::NextTrack,
                                );
                            }
                        });
                        if ui.link("Перейти к вкладке").clicked() {
                            window.activate_webview(view.id());
                            window.set_needs_repaint();
                            ui.close();
                        }
                    }
                });
        })
        .response;
    let rect = egui::Rect::from_center_size(response.rect.center(), egui::Vec2::splat(18.0));
    let color = if playing {
        super::browser_chrome::ACCENT
    } else {
        ui.visuals().weak_text_color()
    };
    let stroke = egui::Stroke::new(1.5_f32, color);
    let p = |x: f32, y: f32| rect.left_top() + egui::vec2(x, y);
    ui.painter()
        .line_segment([p(6.0, 13.0), p(6.0, 3.0)], stroke);
    ui.painter()
        .line_segment([p(6.0, 3.0), p(15.0, 1.0)], stroke);
    ui.painter()
        .line_segment([p(15.0, 1.0), p(15.0, 11.0)], stroke);
    ui.painter().circle_filled(p(3.5, 14.0), 2.5, color);
    ui.painter().circle_filled(p(12.5, 12.0), 2.5, color);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Медиа из всех вкладок")
    });
    response.on_hover_text("Медиа из всех вкладок");
}
