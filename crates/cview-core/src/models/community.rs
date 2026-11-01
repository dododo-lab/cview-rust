//! 채널 커뮤니티 글 — `GET {nng_comment_api}/v1/type/CHANNEL_POST/id/{channelId}/comments` 응답.
//!
//! 서버 응답(`ChannelPostsContent`)은 글·작성자·추천 수가 따로 묶여 있어, 화면에는 한 줄로 편 `ChannelPost` 를 보낸다.

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

/// 응답 `content`. 커뮤니티를 닫은 채널은 `comments` 가 없다.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPostsContent {
    #[serde(default)]
    pub comments: Option<ChannelPostPage>,
    #[serde(default)]
    pub comment_active: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPostPage {
    #[serde(default)]
    pub data: Vec<ChannelPostItem>,
    #[serde(default)]
    pub total_count: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPostItem {
    pub comment: PostComment,
    #[serde(default)]
    pub user: Option<PostUser>,
    #[serde(default)]
    pub buff_nerf: Option<PostBuff>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostComment {
    pub comment_id: i64,
    #[serde(default)]
    pub content: String,
    /// `yyyyMMddHHmmss` (한국 시간).
    #[serde(default)]
    pub created_date: Option<String>,
    #[serde(default)]
    pub attaches: Option<Vec<PostAttach>>,
    /// 댓글 수.
    #[serde(default)]
    pub child_object_count: i64,
    #[serde(default)]
    pub deleted: bool,
    #[serde(default)]
    pub hide_by_clean_bot: bool,
    #[serde(default)]
    pub secret: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostAttach {
    #[serde(default)]
    pub attach_type: String,
    #[serde(default)]
    pub attach_value: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostUser {
    #[serde(default)]
    pub user_nickname: Option<String>,
    #[serde(default)]
    pub profile_image_url: Option<String>,
    #[serde(default)]
    pub title: Option<PostUserTitle>,
    #[serde(default)]
    pub verified_mark: bool,
    #[serde(default)]
    pub verified_mark_type: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PostUserTitle {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostBuff {
    #[serde(default)]
    pub buff_count: i64,
}

/// 화면에 보내는 커뮤니티 글 한 개.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPost {
    pub post_id: i64,
    pub content: String,
    /// ISO 8601(+09:00). 날짜를 읽지 못하면 `None`.
    pub created_at: Option<String>,
    /// 사진 첨부 주소(순서대로).
    pub images: Vec<String>,
    pub comment_count: i64,
    pub like_count: i64,
    pub author_name: String,
    pub author_image_url: Option<String>,
    /// 작성자 칭호(예: 채널 관리자)와 그 색.
    pub author_title: Option<String>,
    pub author_title_color: Option<String>,
    pub author_verified: bool,
    /// 작성자 인증 마크 종류(`"partner"` · `"challenger"` · `"official"`).
    pub author_badge: Option<String>,
}

impl ChannelPostItem {
    /// 지운 글·클린봇이 가린 글·비밀글은 `None` — 목록에 내보이지 않는다.
    pub fn into_post(self) -> Option<ChannelPost> {
        let c = self.comment;
        if c.deleted || c.hide_by_clean_bot || c.secret {
            return None;
        }
        let user = self.user.unwrap_or_default();
        let title = user.title.unwrap_or_default();
        Some(ChannelPost {
            post_id: c.comment_id,
            content: c.content,
            created_at: c.created_date.as_deref().and_then(kst_compact_to_iso),
            images: c
                .attaches
                .unwrap_or_default()
                .into_iter()
                .filter(|a| a.attach_type == "PHOTO" && !a.attach_value.is_empty())
                .map(|a| a.attach_value)
                .collect(),
            comment_count: c.child_object_count.max(0),
            like_count: self.buff_nerf.map_or(0, |b| b.buff_count.max(0)),
            author_name: user.user_nickname.unwrap_or_default(),
            author_image_url: user.profile_image_url.filter(|u| !u.is_empty()),
            author_title: title.name.filter(|n| !n.is_empty()),
            author_title_color: title.color.filter(|c| !c.is_empty()),
            author_verified: user.verified_mark,
            author_badge: super::verified_badge(
                user.verified_mark,
                user.verified_mark_type.as_deref(),
            )
            .map(str::to_string),
        })
    }
}

/// `20260907191415`(한국 시간) → `2026-09-07T19:14:15+09:00`.
fn kst_compact_to_iso(s: &str) -> Option<String> {
    let t = NaiveDateTime::parse_from_str(s, "%Y%m%d%H%M%S").ok()?;
    Some(format!("{}+09:00", t.format("%Y-%m-%dT%H:%M:%S")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_posts_and_hides_deleted_or_secret() {
        let json = r##"{
          "comments": {
            "page": {"next": 1, "prev": null}, "totalCount": 3, "commentCount": 3,
            "data": [
              {"comment": {"commentId": 7, "content": "공지\n둘째 줄", "createdDate": "20260907191415",
                           "childObjectCount": 5, "deleted": false, "hideByCleanBot": false, "secret": false,
                           "attaches": [{"attachType": "PHOTO", "attachValue": "https://img/a.png"},
                                        {"attachType": "STICKER", "attachValue": "x"}]},
               "user": {"userNickname": "관리자", "profileImageUrl": "https://img/p.png",
                        "title": {"name": "채널 관리자", "color": "#749FFE"}, "verifiedMark": false},
               "buffNerf": {"buffCount": 69, "nerfCount": 0}},
              {"comment": {"commentId": 8, "content": "지움", "deleted": true}},
              {"comment": {"commentId": 9, "content": "비밀", "secret": true}}
            ]
          },
          "commentActive": true
        }"##;
        let content: ChannelPostsContent = serde_json::from_str(json).unwrap();
        let page = content.comments.unwrap();
        assert_eq!(page.total_count, Some(3));
        let posts: Vec<_> = page
            .data
            .into_iter()
            .filter_map(ChannelPostItem::into_post)
            .collect();
        assert_eq!(posts.len(), 1);
        let p = &posts[0];
        assert_eq!(p.post_id, 7);
        assert_eq!(p.created_at.as_deref(), Some("2026-09-07T19:14:15+09:00"));
        assert_eq!(p.images, vec!["https://img/a.png".to_string()]);
        assert_eq!((p.comment_count, p.like_count), (5, 69));
        assert_eq!(p.author_title.as_deref(), Some("채널 관리자"));
    }

    #[test]
    fn closed_community_has_no_comments() {
        let content: ChannelPostsContent =
            serde_json::from_str(r#"{"commentActive": false}"#).unwrap();
        assert!(content.comments.is_none());
    }
}
