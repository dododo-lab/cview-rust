//! 채팅 도메인 모델.
//!
//! 원본:
//! - `Sources/CViewCore/Models/ChatModels.swift` → [`messages`]
//! - `Sources/CViewCore/Models/ChatMessageItem.swift` → [`item`]
//! - `Sources/CViewCore/Models/ChatSystemNoticeKind.swift` → [`notice_kind`]
//! - `Sources/CViewCore/Models/EmoticonModels.swift` → [`emoticon`]
//!
//! Swift 의 `URL` 은 이 크레이트가 `url` 의존성을 갖지 않으므로 `String` 으로 옮겼다
//! (`absoluteString` 과 같은 값). `Date` 는 `chrono::DateTime<Utc>` 다.
//! 이모티콘 맵(`[String: String]`)은 `Hash` 를 유지하고 순회 순서를 결정적으로 하기 위해
//! `BTreeMap` 으로 옮겼다.

pub mod emoticon;
pub mod item;
pub mod messages;
pub mod notice_kind;

mod ident;

pub use emoticon::{
    ChatContentSegment, EmoticonDeploy, EmoticonItem, EmoticonPack, EmoticonParser,
};
pub use ident::{is_plausible_url, new_uuid_string};
pub use item::ChatMessageItem;
pub use messages::{
    ChatAccessToken, ChatBadge, ChatConnectionState, ChatExtras, ChatMessage, ChatProfile,
    ChatTitle, DonationInfo, DonationRankItem, DonationRankResponse, EmojiMap, MessageType,
    SubscriptionInfo, TemporaryRestrict, UserRole,
};
pub use notice_kind::ChatSystemNoticeKind;
