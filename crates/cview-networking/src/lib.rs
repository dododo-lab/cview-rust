//! CViewNetworking 포팅: CHZZK API 클라이언트 (`Sources/CViewNetworking`).
//!
//! - [`endpoint`]: 엔드포인트 정의 (`APIEndpoint.swift`)
//! - [`response`]: 응답 래퍼·목록/팔로잉 타입 (`APIResponse.swift`)
//! - [`validator`]: 방어적 디코딩 (`ResponseValidator.swift`)
//! - [`cache`]: 응답 캐시 (`ResponseCache.swift`)
//! - [`gate`]: 전송 속도·동시 수 제한 (`APIRequestGate.swift`)
//! - [`client`]: API 클라이언트 (`ChzzkAPIClient*.swift`)
//! - [`collector`]: 전체 라이브 커서 수집 (`ChzzkAPIClient+LiveCollection.swift`)
//! - [`bootstrap`]: 채팅 세션 부트스트랩 (`ChatSessionBootstrap.swift`)
//! - [`transport`]: HTTP 전송 추상화(reqwest + rustls)
//!
//! 포팅하지 않은 것: 이미지 캐시, 인증서 핀닝(TODO), 요청 장부·픽스처·장애 주입.

pub mod bootstrap;
pub mod cache;
pub mod client;
mod client_content;
mod client_live;
pub mod collector;
pub mod constants;
pub mod endpoint;
pub mod gate;
pub mod headers;
pub mod observer;
pub mod response;
pub mod transport;
pub mod validator;
mod wait;

pub use cache::ResponseCache;
pub use client::{AgedResponse, ChzzkApiClient, ClientConfig, ResponseBody};
pub use client_content::emoticon_family;
pub use client_live::{
    following_max_pages, TraversalOptions, FOLLOWING_PAGE_SIZE, FOLLOWING_ROW_LIMIT,
};
pub use endpoint::{ChzzkEndpoint, TopLivesSortType};
pub use response::{ChzzkResponse, PagedContent};
pub use transport::{HttpRequest, HttpResponse, HttpTransport, ReqwestTransport, TransportError};
