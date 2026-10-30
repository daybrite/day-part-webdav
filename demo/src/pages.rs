// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use crate::{
    model::{Operation, Scene},
    res,
};
use day::prelude::*;
fn status(s: Scene) -> impl Piece {
    column((
        label(s.status).id(day::id!(operation_status)),
        when(
            move || s.busy.get(),
            move || {
                button(res::str::cancel())
                    .action(move || s.cancel())
                    .id(day::id!(cancel_operation))
            },
        ),
    ))
    .spacing(Space::S)
    .align(HAlign::Leading)
}
fn provider(s: Scene) -> AnyPiece {
    #[cfg(all(not(target_arch = "wasm32"), not(target_env = "ohos")))]
    {
        labeled(
            res::str::provider(),
            picker(
                s.provider,
                [
                    (0, res::str::provider_native()),
                    (1, res::str::provider_reqwest()),
                ],
            )
            .segmented()
            .id(day::id!(http_provider)),
        )
        .any()
    }
    #[cfg(any(target_arch = "wasm32", target_env = "ohos"))]
    {
        let _ = s;
        label(res::str::provider_native_only())
            .id(day::id!(native_only))
            .any()
    }
}
pub fn connection() -> impl Piece {
    let s = Scene::ambient();
    scroll(
        column((
            label(res::str::connection_title())
                .font(Font::Title2)
                .bold()
                .id(day::id!(connection_title)),
            label(res::str::connection_help()).secondary(),
            section((
                provider(s),
                labeled(
                    res::str::endpoint(),
                    text_field(s.endpoint)
                        .min_width(180.0)
                        .id(day::id!(dav_endpoint)),
                ),
                labeled(
                    res::str::auth_kind(),
                    picker(
                        s.auth,
                        [
                            (0, res::str::auth_none()),
                            (1, res::str::auth_basic()),
                            (2, res::str::auth_bearer()),
                        ],
                    )
                    .id(day::id!(auth_kind)),
                ),
                labeled(
                    res::str::username(),
                    text_field(s.username)
                        .min_width(180.0)
                        .id(day::id!(dav_username)),
                ),
                labeled(
                    res::str::password(),
                    secure_field(s.password)
                        .min_width(180.0)
                        .id(day::id!(dav_password)),
                ),
                button(res::str::fixture())
                    .action(move || s.fixture())
                    .id(day::id!(use_fixture)),
                label(res::str::fixture_help())
                    .font(Font::Footnote)
                    .secondary(),
            )),
            section((
                labeled(
                    res::str::allow_writes(),
                    toggle(s.writes).id(day::id!(allow_writes)),
                ),
                labeled(
                    res::str::constrained(),
                    toggle(s.constrained).id(day::id!(constrained_network)),
                ),
                labeled(
                    res::str::expensive(),
                    toggle(s.expensive).id(day::id!(allow_expensive)),
                ),
                label(res::str::policy_help())
                    .font(Font::Footnote)
                    .secondary(),
            )),
            row((
                button(res::str::options())
                    .enabled(move || !s.busy.get())
                    .action(move || s.run(Operation::Options))
                    .id(day::id!(server_options)),
                button(res::str::smoke())
                    .enabled(move || !s.busy.get())
                    .action(move || s.run(Operation::Smoke))
                    .id(day::id!(run_checks)),
            ))
            .spacing(Space::S)
            .fit(RowFit::Wrap { run_spacing: 8.0 }),
            label(res::str::smoke_help())
                .font(Font::Footnote)
                .secondary(),
            status(s),
        ))
        .spacing(Space::L)
        .padding(Space::L)
        .align(HAlign::Leading)
        .grow_w(),
    )
}
pub fn browser() -> impl Piece {
    let s = Scene::ambient();
    scroll(
        column((
            label(res::str::nav_browser())
                .font(Font::Title2)
                .bold()
                .id(day::id!(browser_title)),
            label(res::str::browser_help()).secondary(),
            labeled(
                res::str::path(),
                text_field(s.path)
                    .min_width(180.0)
                    .id(day::id!(resource_path)),
            ),
            row((
                button(res::str::browse())
                    .action(move || s.run(Operation::List))
                    .id(day::id!(list_resources)),
                button(res::str::get())
                    .action(move || s.run(Operation::Get))
                    .id(day::id!(download_resource)),
                button(res::str::head())
                    .action(move || s.run(Operation::Head))
                    .id(day::id!(head_resource)),
            ))
            .spacing(Space::S)
            .fit(RowFit::Wrap { run_spacing: 8.0 }),
            when(
                move || s.rows.get().is_empty(),
                || label(res::str::no_resources()).secondary(),
            ),
            list(
                items(
                    move || s.rows.get(),
                    |r: &day_part_webdav::Resource| r.href.clone(),
                ),
                move |slot| {
                    column((
                        label(move || slot.get().href).monospace().single_line(),
                        label(move || {
                            let resource = slot.get();
                            let statuses = resource
                                .status
                                .into_iter()
                                .chain(resource.properties.iter().map(|p| p.status))
                                .map(|s| s.to_string())
                                .collect::<Vec<_>>()
                                .join(", ");
                            format!("{statuses} · {}", resource.etag().unwrap_or_default())
                        })
                        .font(Font::Caption)
                        .secondary(),
                    ))
                    .spacing(Space::XS)
                    .padding(Space::S)
                },
            )
            .row_height(RowHeight::Uniform(64.0))
            .on_select(move |id| s.selected(id))
            .a11y(|a| a.label(res::str::resources()))
            .id(day::id!(resource_list))
            .height(220.0),
            section((
                labeled(
                    res::str::payload(),
                    text_field(s.payload)
                        .min_width(180.0)
                        .id(day::id!(upload_payload)),
                ),
                labeled(
                    res::str::etag(),
                    text_field(s.etag)
                        .min_width(180.0)
                        .id(day::id!(resource_etag)),
                ),
                labeled(
                    res::str::destination(),
                    text_field(s.destination)
                        .min_width(180.0)
                        .id(day::id!(destination_path)),
                ),
                row((
                    button(res::str::create_collection())
                        .enabled(move || s.writes.get())
                        .action(move || s.run(Operation::Mkcol))
                        .id(day::id!(create_collection)),
                    button(res::str::upload())
                        .enabled(move || s.writes.get())
                        .action(move || s.run(Operation::Put))
                        .id(day::id!(upload_resource)),
                    button(res::str::replace())
                        .enabled(move || s.writes.get())
                        .action(move || s.run(Operation::Replace))
                        .id(day::id!(replace_resource)),
                ))
                .spacing(Space::S)
                .fit(RowFit::Wrap { run_spacing: 8.0 }),
                row((
                    button(res::str::copy())
                        .enabled(move || s.writes.get())
                        .action(move || s.run(Operation::Copy))
                        .id(day::id!(copy_resource)),
                    button(res::str::move_resource())
                        .enabled(move || s.writes.get())
                        .action(move || s.run(Operation::Move))
                        .id(day::id!(move_resource)),
                    button(res::str::delete())
                        .enabled(move || s.writes.get())
                        .action(move || s.run(Operation::Delete))
                        .id(day::id!(delete_resource)),
                ))
                .spacing(Space::S)
                .fit(RowFit::Wrap { run_spacing: 8.0 }),
            )),
            row((
                button(res::str::slow())
                    .action(move || s.run(Operation::Slow))
                    .id(day::id!(slow_request)),
                button(res::str::broken())
                    .action(move || s.run(Operation::Broken))
                    .id(day::id!(broken_request)),
            ))
            .spacing(Space::S)
            .fit(RowFit::Wrap { run_spacing: 8.0 }),
            status(s),
        ))
        .spacing(Space::M)
        .padding(Space::L)
        .align(HAlign::Leading)
        .grow_w(),
    )
}
pub fn response() -> impl Piece {
    let s = Scene::ambient();
    scroll(
        column((
            label(res::str::response_title())
                .font(Font::Title2)
                .bold()
                .id(day::id!(response_title)),
            label(res::str::response_help()).secondary(),
            label(res::str::request()).font(Font::Headline),
            label(s.request)
                .monospace()
                .selectable()
                .id(day::id!(request_details)),
            label(res::str::response()).font(Font::Headline),
            label(s.response)
                .monospace()
                .selectable()
                .id(day::id!(response_details)),
            label(res::str::statistics()).font(Font::Headline),
            label(s.statistics).id(day::id!(transfer_statistics)),
        ))
        .spacing(Space::M)
        .padding(Space::L)
        .align(HAlign::Leading)
        .grow_w(),
    )
}
