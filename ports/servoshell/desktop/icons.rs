//! Small vector UI icons, on a shared 24-unit grid, 18px at a 1.5px stroke.
//! Basic Lucide geometry adapted for egui; ISC attribution in design/THIRD-PARTY.md.
use egui::{Color32, Pos2, Rect, Stroke, Ui, Vec2};

#[derive(Clone, Copy)]
pub enum Icon {
    Logo,
    Back,
    Forward,
    Reload,
    Home,
    Star,
    StarFilled,
    Menu,
    Plus,
    Bookmarks,
    History,
    Search,
    Lock,
    LockOpen,
    Info,
    Warning,
    Key,
    Network,
    Settings,
}

pub fn paint(ui: &Ui, rect: Rect, kind: Icon, color: Color32) {
    let s = rect.width().min(rect.height()) / 24.0;
    let p = |x: f32, y: f32| rect.center() + Vec2::new(x - 12.0, y - 12.0) * s;
    let stroke = Stroke::new(1.5_f32, color);
    let line = |points: &[(f32, f32)]| {
        ui.painter().add(egui::Shape::line(
            points.iter().map(|&(x, y)| p(x, y)).collect::<Vec<Pos2>>(),
            stroke,
        ))
    };
    match kind {
        Icon::Lock | Icon::LockOpen => {
            ui.painter().rect_stroke(
                Rect::from_two_pos(p(5., 10.), p(19., 22.)),
                2.0 * s,
                stroke,
                egui::StrokeKind::Inside,
            );
            if matches!(kind, Icon::Lock) {
                let points = (0..=12)
                    .map(|i| {
                        let angle = std::f32::consts::PI * i as f32 / 12.;
                        p(12. + 5. * angle.cos(), 9. - 6. * angle.sin())
                    })
                    .collect();
                ui.painter().add(egui::Shape::line(points, stroke));
                line(&[(7., 11.), (7., 9.)]);
                line(&[(17., 9.), (17., 11.)]);
            } else {
                line(&[
                    (7., 11.),
                    (7., 7.),
                    (8., 4.),
                    (11., 3.),
                    (14., 3.),
                    (17., 5.),
                    (17., 6.),
                ]);
            }
            ui.painter().circle_filled(p(12., 15.), 1.2 * s, color);
            line(&[(12., 15.), (12., 18.)]);
        },
        Icon::Info => {
            ui.painter().circle_stroke(p(12., 12.), 9. * s, stroke);
            ui.painter().circle_filled(p(12., 7.), 0.9 * s, color);
            line(&[(12., 11.), (12., 17.)]);
        },
        Icon::Warning => {
            line(&[(12., 3.), (22., 21.), (2., 21.), (12., 3.)]);
            line(&[(12., 9.), (12., 14.)]);
            ui.painter().circle_filled(p(12., 17.5), 0.9 * s, color);
        },
        Icon::Search => {
            ui.painter().circle_stroke(p(10., 10.), 6.0 * s, stroke);
            line(&[(14.5, 14.5), (21., 21.)]);
        },
        Icon::Logo => {
            let shield = [
                (12., 1.5),
                (21., 5.25),
                (19.5, 15.75),
                (12., 22.5),
                (4.5, 15.75),
                (3., 5.25),
            ]
            .into_iter()
            .map(|(x, y)| p(x, y))
            .collect();
            ui.painter().add(egui::Shape::convex_polygon(
                shield,
                color,
                egui::Stroke::NONE,
            ));
            let dark = egui::Stroke::new(1.8_f32, Color32::from_rgb(23, 23, 25));
            ui.painter().add(egui::Shape::closed_line(
                [(7.5, 8.25), (12., 6.), (16.5, 8.25), (12., 16.5)]
                    .into_iter()
                    .map(|(x, y)| p(x, y))
                    .collect(),
                dark,
            ));
        },
        Icon::Back => {
            line(&[(12., 5.), (5., 12.), (12., 19.)]);
            line(&[(5., 12.), (21., 12.)]);
        },
        Icon::Forward => {
            line(&[(12., 5.), (19., 12.), (12., 19.)]);
            line(&[(3., 12.), (19., 12.)]);
        },
        Icon::Reload => {
            let points = (0..=22)
                .map(|i| {
                    let a = (45.0 + i as f32 * 14.0).to_radians();
                    p(12. + a.cos() * 8., 12. + a.sin() * 8.)
                })
                .collect::<Vec<_>>();
            ui.painter().add(egui::Shape::line(points, stroke));
            line(&[(20., 3.), (20., 9.), (14., 9.)]);
        },
        Icon::Home => {
            line(&[(3., 10.), (12., 3.), (21., 10.)]);
            line(&[
                (5., 9.),
                (5., 21.),
                (10., 21.),
                (10., 14.),
                (14., 14.),
                (14., 21.),
                (19., 21.),
                (19., 9.),
            ]);
        },
        Icon::Star | Icon::StarFilled => {
            let points = (0..10)
                .map(|i| {
                    let a = (i as f32 * 36. - 90.).to_radians();
                    let r = if i % 2 == 0 { 10. } else { 4.5 };
                    p(12. + r * a.cos(), 12. + r * a.sin())
                })
                .collect::<Vec<_>>();
            if matches!(kind, Icon::StarFilled) {
                for i in 0..points.len() {
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![rect.center(), points[i], points[(i + 1) % points.len()]],
                        color,
                        egui::Stroke::NONE,
                    ));
                }
                ui.painter().add(egui::Shape::closed_line(points, stroke));
            } else {
                ui.painter().add(egui::Shape::closed_line(points, stroke));
            }
        },
        Icon::Menu => {
            for y in [6., 12., 18.] {
                line(&[(4., y), (20., y)]);
            }
        },
        Icon::Plus => {
            line(&[(5., 12.), (19., 12.)]);
            line(&[(12., 5.), (12., 19.)]);
        },
        Icon::Bookmarks => {
            line(&[
                (6., 3.),
                (18., 3.),
                (18., 21.),
                (12., 17.),
                (6., 21.),
                (6., 3.),
            ]);
        },
        Icon::History => {
            ui.painter().circle_stroke(p(12., 12.), 8. * s, stroke);
            line(&[(12., 7.), (12., 12.), (16., 14.)]);
        },
        Icon::Key => {
            ui.painter().circle_stroke(p(8., 8.), 5. * s, stroke);
            line(&[(11.5, 11.5), (21., 21.), (21., 17.)]);
            line(&[(17., 17.), (20., 14.)]);
        },
        Icon::Network => {
            ui.painter().circle_stroke(p(12., 12.), 9. * s, stroke);
            line(&[(3., 12.), (21., 12.)]);
            line(&[
                (12., 3.),
                (8., 7.),
                (7., 12.),
                (8., 17.),
                (12., 21.),
                (16., 17.),
                (17., 12.),
                (16., 7.),
                (12., 3.),
            ]);
        },
        Icon::Settings => {
            for (y, x) in [(6., 9.), (12., 16.), (18., 7.)] {
                line(&[(3., y), (x - 2., y)]);
                line(&[(x + 2., y), (21., y)]);
                ui.painter().circle_stroke(p(x, y), 2. * s, stroke);
            }
        },
    }
}

pub fn button(ui: &mut Ui, kind: Icon, title: &str, enabled: bool) -> egui::Response {
    let response = ui.add_enabled(
        enabled,
        egui::Button::new("")
            .frame(false)
            .min_size(Vec2::splat(32.0)),
    );
    let color = if enabled {
        ui.visuals().weak_text_color()
    } else {
        ui.visuals().weak_text_color().gamma_multiply(0.35)
    };
    if response.hovered() || response.is_pointer_button_down_on() {
        ui.painter().rect_filled(
            response.rect,
            8,
            if response.is_pointer_button_down_on() {
                ui.visuals().widgets.active.weak_bg_fill
            } else {
                ui.visuals().widgets.hovered.weak_bg_fill
            },
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.shrink(1.0),
            8,
            egui::Stroke::new(1.5_f32, ui.visuals().hyperlink_color),
            egui::StrokeKind::Inside,
        );
    }
    paint(
        ui,
        Rect::from_center_size(response.rect.center(), Vec2::splat(18.0)),
        kind,
        color,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, title));
    response.on_hover_text(title)
}

pub fn menu_row(ui: &mut Ui, kind: Icon, title: &str, shortcut: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(272.0, 36.0), egui::Sense::click());
    if response.hovered() || response.has_focus() {
        ui.painter()
            .rect_filled(rect, 6, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    paint(
        ui,
        Rect::from_center_size(rect.left_center() + Vec2::new(20.0, 0.0), Vec2::splat(18.0)),
        kind,
        ui.visuals().weak_text_color(),
    );
    ui.painter().text(
        rect.left_center() + Vec2::new(40.0, 0.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(14.0),
        ui.visuals().text_color(),
    );
    ui.painter().text(
        rect.right_center() - Vec2::new(12.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        shortcut,
        egui::FontId::proportional(12.0),
        ui.visuals().weak_text_color(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, title));
    response
}
