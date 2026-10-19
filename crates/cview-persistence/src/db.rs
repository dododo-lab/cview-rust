//! 행 ↔ 구조체 매핑 보조(SwiftData `ModelContext` 의 fetch/insert/delete 자리).
//!
//! 원본: 각 `Models/*.swift` 의 @Model 클래스와 `DataStore*.swift` 의 `FetchDescriptor` 사용처.
//! SwiftData 는 모델 객체를 수정한 뒤 `save()` 로 커밋했다. 이 포팅은 행을 구조체로 읽고,
//! 바꾼 구조체만 `update` 로 되쓴 뒤 트랜잭션을 커밋한다(`store::DataStore::write`).

use crate::time::{from_db, to_db, Date};
use rusqlite::types::{Value, ValueRef};
use rusqlite::{Connection, Row};

/// 열 하나의 값 변환.
pub(crate) trait Col: Sized {
    fn value(&self) -> Value;
    fn read(row: &Row<'_>, col: &str) -> rusqlite::Result<Self>;
}

impl Col for String {
    fn value(&self) -> Value {
        Value::Text(self.clone())
    }
    fn read(row: &Row<'_>, col: &str) -> rusqlite::Result<Self> {
        row.get(col)
    }
}

impl Col for Option<String> {
    fn value(&self) -> Value {
        self.clone().map_or(Value::Null, Value::Text)
    }
    fn read(row: &Row<'_>, col: &str) -> rusqlite::Result<Self> {
        row.get(col)
    }
}

impl Col for i64 {
    fn value(&self) -> Value {
        Value::Integer(*self)
    }
    fn read(row: &Row<'_>, col: &str) -> rusqlite::Result<Self> {
        row.get(col)
    }
}

impl Col for Option<i64> {
    fn value(&self) -> Value {
        self.map_or(Value::Null, Value::Integer)
    }
    fn read(row: &Row<'_>, col: &str) -> rusqlite::Result<Self> {
        row.get(col)
    }
}

/// SQLite 는 NaN 을 NULL 로 저장해 NOT NULL 제약을 깨뜨린다 — 0 으로 둔다
/// (집계 쪽은 원본처럼 비유한 값을 0 으로 다룬다).
fn finite_or_zero(v: f64) -> f64 {
    if v.is_nan() {
        0.0
    } else {
        v
    }
}

impl Col for f64 {
    fn value(&self) -> Value {
        Value::Real(finite_or_zero(*self))
    }
    fn read(row: &Row<'_>, col: &str) -> rusqlite::Result<Self> {
        row.get(col)
    }
}

impl Col for Option<f64> {
    fn value(&self) -> Value {
        self.map_or(Value::Null, |v| Value::Real(finite_or_zero(v)))
    }
    fn read(row: &Row<'_>, col: &str) -> rusqlite::Result<Self> {
        row.get(col)
    }
}

impl Col for bool {
    fn value(&self) -> Value {
        Value::Integer(i64::from(*self))
    }
    fn read(row: &Row<'_>, col: &str) -> rusqlite::Result<Self> {
        row.get(col)
    }
}

impl Col for Vec<u8> {
    fn value(&self) -> Value {
        Value::Blob(self.clone())
    }
    fn read(row: &Row<'_>, col: &str) -> rusqlite::Result<Self> {
        match row.get_ref(col)? {
            ValueRef::Blob(b) => Ok(b.to_vec()),
            ValueRef::Text(t) => Ok(t.to_vec()),
            ValueRef::Null => Ok(Vec::new()),
            other => Err(rusqlite::Error::InvalidColumnType(
                0,
                col.to_string(),
                other.data_type(),
            )),
        }
    }
}

impl Col for Date {
    fn value(&self) -> Value {
        Value::Integer(to_db(self))
    }
    fn read(row: &Row<'_>, col: &str) -> rusqlite::Result<Self> {
        Ok(from_db(row.get::<_, i64>(col)?))
    }
}

impl Col for Option<Date> {
    fn value(&self) -> Value {
        self.as_ref()
            .map_or(Value::Null, |d| Value::Integer(to_db(d)))
    }
    fn read(row: &Row<'_>, col: &str) -> rusqlite::Result<Self> {
        Ok(row.get::<_, Option<i64>>(col)?.map(from_db))
    }
}

/// 날짜 매개변수.
pub(crate) fn dv(date: &Date) -> Value {
    Value::Integer(to_db(date))
}

/// 테이블에 대응하는 모델 구조체.
pub(crate) trait Model: Sized {
    const TABLE: &'static str;
    /// `id` 를 뺀 열 이름(선언 순서).
    const COLUMNS: &'static [&'static str];
    fn from_row(row: &Row<'_>) -> rusqlite::Result<Self>;
    fn values(&self) -> Vec<Value>;
    fn row_id(&self) -> i64;
    fn set_row_id(&mut self, id: i64);
}

/// `SELECT * FROM <table> <suffix>`.
pub(crate) fn fetch<M: Model>(
    conn: &Connection,
    suffix: &str,
    params: &[Value],
) -> rusqlite::Result<Vec<M>> {
    let sql = format!("SELECT * FROM {} {}", M::TABLE, suffix);
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), M::from_row)?;
    rows.collect()
}

/// 첫 행.
pub(crate) fn fetch_one<M: Model>(
    conn: &Connection,
    suffix: &str,
    params: &[Value],
) -> rusqlite::Result<Option<M>> {
    Ok(fetch::<M>(conn, &format!("{suffix} LIMIT 1"), params)?
        .into_iter()
        .next())
}

/// `SELECT count(*) FROM <table> <suffix>`.
pub(crate) fn count<M: Model>(
    conn: &Connection,
    suffix: &str,
    params: &[Value],
) -> rusqlite::Result<i64> {
    let sql = format!("SELECT count(*) FROM {} {}", M::TABLE, suffix);
    conn.prepare_cached(&sql)?
        .query_row(rusqlite::params_from_iter(params.iter()), |r| r.get(0))
}

/// 삽입 후 행 식별자를 채운다.
pub(crate) fn insert<M: Model>(conn: &Connection, model: &mut M) -> rusqlite::Result<()> {
    let placeholders = vec!["?"; M::COLUMNS.len()].join(",");
    let sql = format!(
        "INSERT INTO {} ({}) VALUES ({})",
        M::TABLE,
        M::COLUMNS.join(","),
        placeholders
    );
    conn.prepare_cached(&sql)?
        .execute(rusqlite::params_from_iter(model.values()))?;
    model.set_row_id(conn.last_insert_rowid());
    Ok(())
}

/// 행 전체를 되쓴다.
pub(crate) fn update<M: Model>(conn: &Connection, model: &M) -> rusqlite::Result<()> {
    let sets = M::COLUMNS
        .iter()
        .map(|c| format!("{c} = ?"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("UPDATE {} SET {} WHERE id = ?", M::TABLE, sets);
    let mut values = model.values();
    values.push(Value::Integer(model.row_id()));
    conn.prepare_cached(&sql)?
        .execute(rusqlite::params_from_iter(values))?;
    Ok(())
}

/// 행 삭제.
pub(crate) fn delete<M: Model>(conn: &Connection, model: &M) -> rusqlite::Result<()> {
    let sql = format!("DELETE FROM {} WHERE id = ?", M::TABLE);
    conn.prepare_cached(&sql)?.execute([model.row_id()])?;
    Ok(())
}

/// `?,?,?` 자리표시자 n 개.
pub(crate) fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// IN 조회를 청크로 나눠 모은다(SQLite 바인딩 한계 회피).
pub(crate) fn fetch_in<M: Model>(
    conn: &Connection,
    column: &str,
    keys: &[Value],
    chunk_size: usize,
) -> rusqlite::Result<Vec<M>> {
    let mut out = Vec::new();
    for chunk in keys.chunks(chunk_size.max(1)) {
        let suffix = format!("WHERE {column} IN ({})", placeholders(chunk.len()));
        out.extend(fetch::<M>(conn, &suffix, chunk)?);
    }
    Ok(out)
}

/// 모델 구조체와 [`Model`] 구현을 만든다. 필드마다 SQL 열 이름을 적는다.
macro_rules! db_model {
    (
        $(#[$meta:meta])*
        pub struct $name:ident in $table:literal {
            $( $(#[$fmeta:meta])* pub $field:ident : $ty:ty => $col:literal ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $name {
            /// SQLite 행 식별자(SwiftData `PersistentIdentifier` 자리). 삽입 전에는 0.
            pub id: i64,
            $( $(#[$fmeta])* pub $field: $ty, )*
        }

        impl $crate::db::Model for $name {
            const TABLE: &'static str = $table;
            const COLUMNS: &'static [&'static str] = &[$($col),*];
            fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
                Ok(Self {
                    id: row.get("id")?,
                    $( $field: <$ty as $crate::db::Col>::read(row, $col)?, )*
                })
            }
            fn values(&self) -> Vec<rusqlite::types::Value> {
                vec![$( <$ty as $crate::db::Col>::value(&self.$field) ),*]
            }
            fn row_id(&self) -> i64 {
                self.id
            }
            fn set_row_id(&mut self, id: i64) {
                self.id = id;
            }
        }
    };
}
pub(crate) use db_model;

/// CSV source 문자열을 집합으로(빈 토큰 제외).
pub(crate) fn split_sources(raw: &str) -> std::collections::BTreeSet<String> {
    raw.split(',')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// 집합을 정렬 CSV 로(`sorted().joined(separator: ",")`).
pub(crate) fn join_sources<'a>(sources: impl IntoIterator<Item = &'a String>) -> String {
    let mut v: Vec<&String> = sources.into_iter().collect();
    v.sort();
    v.dedup();
    v.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(",")
}
