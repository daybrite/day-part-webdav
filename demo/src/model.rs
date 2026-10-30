// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0
use crate::{Section, res};
use day::prelude::*;
use day_part_webdav::Resource as DavResource;
use day_part_webdav::{
    self as dav,
    http::{Provider, Session},
    *,
};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Clone, Copy)]
pub(crate) struct Scene {
    pub section: Signal<Section>,
    pub endpoint: Signal<String>,
    pub username: Signal<String>,
    pub password: Signal<String>,
    pub auth: Signal<usize>,
    #[cfg(all(not(target_arch = "wasm32"), not(target_env = "ohos")))]
    pub provider: Signal<usize>,
    pub writes: Signal<bool>,
    pub constrained: Signal<bool>,
    pub expensive: Signal<bool>,
    pub path: Signal<String>,
    pub destination: Signal<String>,
    pub payload: Signal<String>,
    pub etag: Signal<String>,
    pub session: Signal<Session>,
    pub status: Signal<String>,
    pub busy: Signal<bool>,
    pub task: Signal<Option<day::TaskHandle>>,
    pub request: Signal<String>,
    pub response: Signal<String>,
    pub rows: Signal<Vec<DavResource>>,
    pub statistics: Signal<String>,
}
impl Ambient for Scene {
    fn create() -> Self {
        Self {
            section: Signal::new(Section::Connection),
            endpoint: Signal::new(fixture_url().unwrap_or_default()),
            username: Signal::new(String::new()),
            password: Signal::new(String::new()),
            auth: Signal::new(0),
            #[cfg(all(not(target_arch = "wasm32"), not(target_env = "ohos")))]
            provider: Signal::new(0),
            writes: Signal::new(true),
            constrained: Signal::new(false),
            expensive: Signal::new(false),
            path: Signal::new(String::new()),
            destination: Signal::new(String::new()),
            payload: Signal::new(res::str::fixture_payload().format()),
            etag: Signal::new(String::new()),
            session: Signal::new(Session::new()),
            status: Signal::new(res::str::status_ready().format()),
            busy: Signal::new(false),
            task: Signal::new(None),
            request: Signal::new(String::new()),
            response: Signal::new(String::new()),
            rows: Signal::new(vec![]),
            statistics: Signal::new(String::new()),
        }
    }
}
fn fixture_url() -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static SERVER: std::sync::OnceLock<Option<dav::testing::Server>> =
            std::sync::OnceLock::new();
        SERVER
            .get_or_init(|| dav::testing::Server::start().ok())
            .as_ref()
            .map(|s| s.url())
    }
    #[cfg(target_arch = "wasm32")]
    {
        Some("http://127.0.0.1:18444/dav/".into())
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum Operation {
    Options,
    List,
    Get,
    Head,
    Mkcol,
    Put,
    Replace,
    Copy,
    Move,
    Delete,
    Smoke,
    Slow,
    Broken,
}
impl Operation {
    fn writes(self) -> bool {
        matches!(
            self,
            Self::Mkcol
                | Self::Put
                | Self::Replace
                | Self::Copy
                | Self::Move
                | Self::Delete
                | Self::Smoke
        )
    }
}
pub fn relative(input: &str, collection: bool) -> Result<ResourcePath, Error> {
    let input = input.trim_matches('/');
    if input.is_empty() {
        Ok(ResourcePath::root())
    } else {
        ResourcePath::from_segments(input.split('/'), collection)
    }
}
impl Scene {
    pub fn fixture(self) {
        self.cancel();
        if let Some(url) = fixture_url() {
            self.endpoint.set(url);
            self.username.set(String::new());
            self.password.set(String::new());
            self.auth.set(0);
            self.path.set(String::new());
            self.writes.set(true);
            self.status.set(res::str::status_ready().format());
        } else {
            self.status.set(res::str::fixture_failed().format());
        }
    }
    pub fn cancel(self) {
        if let Some(task) = self.task.get_untracked() {
            task.abort();
        }
        self.task.set(None);
        self.busy.set(false);
        self.status.set(res::str::status_cancelled().format());
    }
    pub fn selected(self, id: String) {
        if let Some(resource) = self.rows.get_untracked().iter().find(|r| r.href == id) {
            if let Some(relative) = resource.href.strip_prefix(&self.endpoint.get_untracked()) {
                self.path.set(
                    percent_encoding::percent_decode_str(relative)
                        .decode_utf8_lossy()
                        .into_owned(),
                );
            }
            self.etag.set(resource.etag().unwrap_or_default().into());
            self.response.set(format!("{resource:#?}"));
        }
    }
    fn client(self) -> Result<Client, Error> {
        let raw = self.endpoint.get_untracked();
        let endpoint = Endpoint::new(&raw).or_else(|_| Endpoint::loopback(&raw))?;
        // Store the normalized base so resource selection uses the same confinement boundary.
        self.endpoint.set(endpoint.as_str().into());
        let session = self.session.get_untracked();
        #[cfg(all(not(target_arch = "wasm32"), not(target_env = "ohos")))]
        session.set_provider(if self.provider.get_untracked() == 1 {
            Provider::Reqwest
        } else {
            Provider::Native
        });
        #[cfg(any(target_arch = "wasm32", target_env = "ohos"))]
        session.set_provider(Provider::Native);
        let authentication = match self.auth.get_untracked() {
            1 => Authentication::Basic {
                username: self.username.get_untracked(),
                password: self.password.get_untracked(),
            },
            2 => Authentication::Bearer(self.password.get_untracked()),
            _ => Authentication::None,
        };
        let policy = Policy {
            max_response_bytes: 1024 * 1024,
            max_upload_bytes: 1024 * 1024,
            allow_expensive: self.expensive.get_untracked(),
            ..Default::default()
        };
        let client = Client::builder(endpoint)
            .session(session)
            .authentication(authentication)
            .policy(policy)
            .build();
        if self.constrained.get_untracked() {
            client.set_network_state(NetworkState::Constrained);
        }
        Ok(client)
    }
    pub fn run(self, op: Operation) {
        self.cancel();
        if op.writes() && !self.writes.get_untracked() {
            self.status.set(res::str::write_disabled().format());
            return;
        }
        let client = match self.client() {
            Ok(c) => c,
            Err(_) => {
                self.status.set(res::str::endpoint_invalid().format());
                return;
            }
        };
        self.busy.set(true);
        self.status.set(res::str::status_working().format());
        self.response.set(String::new());
        let input = self.path.get_untracked();
        let dest = self.destination.get_untracked();
        let body = self.payload.get_untracked().into_bytes();
        let etag = self.etag.get_untracked();
        // Protocol operation/URL are inspection data, not app-owned labels. Credentials excluded.
        self.request
            .set(format!("{op:?}\n{}\n{input}", client.endpoint().as_str()));
        let handle = day::task(async move {
            let result = execute(&client, op, &input, &dest, body, &etag).await;
            self.busy.set(false);
            match result {
                Ok(output) => {
                    self.status.set(if matches!(op, Operation::Smoke) {
                        res::str::smoke_ok().format()
                    } else {
                        res::str::status_ok().format()
                    });
                    self.response.set(output.text);
                    if let Some(rows) = output.rows {
                        self.rows.set(rows);
                    }
                }
                Err(e) => {
                    self.status.set(if matches!(e, Error::Deferred(_)) {
                        res::str::status_deferred().format()
                    } else {
                        res::str::status_error(e.to_string()).format()
                    });
                    self.response.set(format!("{e:#?}"));
                }
            }
            let stats = self.session.get_untracked().statistics();
            self.statistics.set(
                res::str::transfer_summary(res::str::TransferSummary {
                    count: stats.started.to_string(),
                    uploaded: stats.uploaded_bytes.to_string(),
                    downloaded: stats.downloaded_bytes.to_string(),
                })
                .format(),
            );
        });
        self.task.set(Some(handle));
        Scope::current().on_cleanup(move || handle.abort());
    }
}
struct Output {
    text: String,
    rows: Option<Vec<DavResource>>,
}
async fn execute(
    client: &Client,
    op: Operation,
    path: &str,
    destination: &str,
    body: Vec<u8>,
    etag: &str,
) -> Result<Output, Error> {
    let path = relative(path, matches!(op, Operation::List | Operation::Mkcol))?;
    let text = match op {
        Operation::Smoke => return smoke(client).await,
        Operation::Options => format!("{:#?}", client.options().await?),
        Operation::List => {
            let result = client.propfind(&path, Depth::One).await?;
            return Ok(Output {
                text: format!("{result:#?}"),
                rows: Some(result.resources),
            });
        }
        _ => {
            let response = match op {
                Operation::Get => client.get(&path, None).await?,
                Operation::Head => client.head(&path).await?,
                Operation::Mkcol => client.mkcol(&path).await?,
                Operation::Put => client.put(&path, body, Condition::CreateOnly).await?,
                Operation::Replace => {
                    client
                        .put(&path, body, Condition::IfMatch(EntityTag::parse(etag)?))
                        .await?
                }
                Operation::Copy => {
                    client
                        .copy(&path, &relative(destination, false)?, false)
                        .await?
                }
                Operation::Move => {
                    client
                        .move_resource(&path, &relative(destination, false)?, false)
                        .await?
                }
                Operation::Delete => {
                    if path.as_encoded_str().is_empty() {
                        return Err(Error::InvalidPath);
                    }
                    client.delete(&path, Condition::Unconditional).await?
                }
                Operation::Slow => client.get(&relative("__slow__", false)?, None).await?,
                Operation::Broken => client.get(&relative("__drop__", false)?, None).await?,
                _ => unreachable!(),
            };
            let headers = response
                .headers
                .iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.to_ascii_lowercase().as_str(),
                        "set-cookie" | "authorization" | "proxy-authenticate"
                    )
                })
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "HTTP {}\n{headers}\n\n{}",
                response.status,
                String::from_utf8_lossy(&response.body[..response.body.len().min(16384)])
            )
        }
    };
    Ok(Output { text, rows: None })
}
async fn smoke(client: &Client) -> Result<Output, Error> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let suffix = format!(
        "day-fixture-{}-{}",
        dav::http::simulation::Clock::now(&dav::http::simulation::RealClock).as_nanos(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    );
    let dir = ResourcePath::from_segments([suffix.as_str()], true)?;
    let file = ResourcePath::from_segments([suffix.as_str(), "café + 100%.bin"], false)?;
    let copy = ResourcePath::from_segments([suffix.as_str(), "copy.bin"], false)?;
    let moved = ResourcePath::from_segments([suffix.as_str(), "moved.bin"], false)?;
    client.options().await?;
    client.mkcol(&dir).await?;
    // Cleanup only our successfully created collection, including when a check fails.
    let result=async {
        let bytes=b"Synthetic protocol fixture\x00\xff".to_vec();client.put(&file,bytes.clone(),Condition::CreateOnly).await?;
        let listed=client.propfind(&dir,Depth::One).await?;if listed.resources.len()!=2{return Err(Error::Protocol);}
        let response=client.get(&file,None).await?;if response.body!=bytes{return Err(Error::Protocol);}
        let tag=EntityTag::parse(response.header("etag").ok_or(Error::Protocol)?)?;
        if client.get(&file,Some(&tag)).await?.status!=304{return Err(Error::Protocol);}
        client.head(&file).await?;
        if !matches!(client.put(&file,vec![0],Condition::CreateOnly).await,Err(Error::Status {status:412,..})){return Err(Error::Protocol);}
        if tag.is_weak(){return Err(Error::InvalidEntityTag);}
        if !matches!(client.get_range(&file,1,4,&tag).await?,RangeDownload::Partial {response,..} if response.body==b"ynth"){return Err(Error::Protocol);}
        client.copy(&file,&copy,false).await?;client.move_resource(&copy,&moved,false).await?;
        if client.get(&moved,None).await?.body!=bytes{return Err(Error::Protocol);}
        client.delete(&moved,Condition::Unconditional).await?;
        if !matches!(client.get(&moved,None).await,Err(Error::Status {status:404,..})){return Err(Error::Protocol);}
        Ok(Output {text:format!("OPTIONS → MKCOL → PUT → PROPFIND → GET → HEAD → GET 304 → PUT 412 → RANGE → COPY → MOVE → DELETE → GET 404\n{listed:#?}"),rows:Some(vec![])})
    }.await;
    let cleanup = client.delete(&dir, Condition::Unconditional).await;
    match (result, cleanup) {
        (Ok(output), Ok(_)) => Ok(output),
        (Err(e), _) => Err(e),
        (_, Err(e)) => Err(e),
    }
}
