// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use day::prelude::*;
use day_piece_settings::prelude::*;
mod model;
mod pages;
use model::Scene;
day::resources!();
day::day_start!(options: res::window_options(), root);
day::routes! {pub(crate) enum Section {Connection=>"connection",Browser=>"browser",Response=>"response",Settings=>"settings"}}
pub fn root() -> impl Piece {
    let preferences = Preferences::new()
        .about(About::from_manifest().website("https://daybrite.dev"))
        .group(general_group(res::locales::ALL))
        .install();
    Scene::scoped(move |scene| {
        nav(scene.section)
            .title(res::str::app_title())
            .items((
                item(Section::Connection, res::str::nav_connection())
                    .icon(res::vectors::tab_welcome)
                    .page(pages::connection),
                item(Section::Browser, res::str::nav_browser())
                    .icon(res::vectors::tab_items)
                    .page(pages::browser),
                item(Section::Response, res::str::nav_response())
                    .icon(res::vectors::tab_items)
                    .page(pages::response),
                preferences
                    .nav_item(Section::Settings)
                    .map(|i| i.icon(res::vectors::tab_settings)),
            ))
            .id(day::id!(nav))
    })
}
