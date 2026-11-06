// Differential tests: //crates/mln_storage and //crates/mln_sprite against
// the C++ core.
//
// 1. Every row of metrics/cache-style.db and metrics/cache-metrics.db (plus
//    edge cases: missing keys, data ranges, other pixel ratios, volatile
//    resources) through `OfflineDatabase::get` (read-only, as the render
//    tests open it) and through `DatabaseFileSource::request` on a RunLoop:
//    identical responses (decompressed data bytes, expires/modified/etag,
//    mustRevalidate, noContent, error reason + message) and log records.
// 2. `parseSprite` on the sprite fixtures (test/fixtures, the sprites stored
//    in the cache databases) and on generated sprite JSON: identical image
//    ids, sizes, pixel ratios, sdf, stretches, content, textFit, pixels,
//    errors and log records. JSON syntax errors only need the
//    "Failed to parse JSON: " prefix (serde_json vs. rapidjson messages,
//    DEVIATIONS.md); image decoder errors only need to be errors on both
//    sides (libpng vs. png crate texts, DEVIATIONS.md).
//
// The databases are copied to TEST_TMPDIR first; neither side ever opens
// the C++ repository's files.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use mln_actor::RunLoop;
use mln_sprite::parse_sprite;
use mln_storage::response::Reason;
use mln_storage::{
    ClientOptions, DatabaseFileSource, FileSource, Kind, LoadingMethod, OfflineDatabase, READ_ONLY_MODE_KEY, Resource,
    ResourceOptions, Response, StoragePolicy, TileData,
};
use mln_util::logging::{Event, EventSeverity, Log, Observer};
use mln_util::tile_server_options::TileServerOptions;
use proptest::prelude::*;

/// The C++ oracle process (oracle_main.cpp), shared by the tests: one call
/// at a time.
struct Oracle {
    _child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: std::process::ChildStdout,
}

static ORACLE: Mutex<Option<Oracle>> = Mutex::new(None);

/// Sends `op` + `payload` to the oracle and returns its answer.
fn oracle_call(op: u8, payload: &[u8]) -> Vec<u8> {
    use std::io::{Read, Write};
    let mut guard = ORACLE.lock().unwrap_or_else(|e| e.into_inner());
    let oracle = guard.get_or_insert_with(|| {
        let base = std::env::var("RUNFILES_DIR").or_else(|_| std::env::var("TEST_SRCDIR")).expect("RUNFILES_DIR");
        let path = PathBuf::from(base).join("_main/difftest/storage/storage_oracle");
        let mut child = std::process::Command::new(path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("start the C++ oracle");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        Oracle { _child: child, stdin, stdout }
    });
    oracle.stdin.write_all(&[op]).unwrap();
    oracle.stdin.write_all(&(payload.len() as u32).to_le_bytes()).unwrap();
    oracle.stdin.write_all(payload).unwrap();
    oracle.stdin.flush().unwrap();
    let mut len = [0u8; 4];
    oracle.stdout.read_exact(&mut len).unwrap_or_else(|e| panic!("the C++ oracle exited (assert?): {e}"));
    let mut answer = vec![0u8; u32::from_le_bytes(len) as usize];
    oracle.stdout.read_exact(&mut answer).unwrap();
    answer
}

/// Serializes the tests: the Rust side captures logs per thread, but the
/// DatabaseFileSource tests share the process-wide FileSourceManager.
static TEST_LOCK: Mutex<()> = Mutex::new(());

// ---- encoding (see reference.cpp) ----

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, n: usize) -> &[u8] {
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        head
    }
    fn u8(&mut self) -> u8 {
        self.take(1)[0]
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn i64(&mut self) -> i64 {
        i64::from_le_bytes(self.take(8).try_into().unwrap())
    }
    fn f32(&mut self) -> f32 {
        f32::from_le_bytes(self.take(4).try_into().unwrap())
    }
    fn bytes(&mut self) -> Vec<u8> {
        let n = self.u32() as usize;
        self.take(n).to_vec()
    }
    fn string(&mut self) -> String {
        String::from_utf8_lossy(&self.bytes()).into_owned()
    }
}

fn write_resource(w: &mut Writer, r: &Resource) {
    w.u8(r.kind as u8);
    w.u8(r.loading_method.0);
    w.u8(match r.storage_policy {
        StoragePolicy::Permanent => 0,
        StoragePolicy::Volatile => 1,
    });
    w.bytes(r.url.as_bytes());
    match &r.tile_data {
        Some(t) => {
            w.u8(1);
            w.bytes(t.url_template.as_bytes());
            w.u8(t.pixel_ratio);
            w.i32(t.x);
            w.i32(t.y);
            w.u8(t.z as u8);
        }
        None => w.u8(0),
    }
    match r.data_range {
        Some((a, b)) => {
            w.u8(1);
            w.u64(a);
            w.u64(b);
        }
        None => w.u8(0),
    }
}

fn encode_resources(resources: &[Resource]) -> Vec<u8> {
    let mut w = Writer::default();
    w.u32(resources.len() as u32);
    for r in resources {
        write_resource(&mut w, r);
    }
    w.0
}

/// Bytes whose Debug output is short (length + FNV-1a hash).
#[derive(Clone, PartialEq)]
struct Bytes(Vec<u8>);

impl std::fmt::Debug for Bytes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hash = self.0.iter().fold(0xcbf29ce484222325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100000001b3));
        write!(f, "<{} bytes, fnv {hash:016x}>", self.0.len())
    }
}

/// A response as both sides report it (`None`: `std::nullopt`).
#[derive(Clone, Debug, PartialEq)]
struct ResponseDump {
    error: Option<(u8, String)>,
    no_content: bool,
    not_modified: bool,
    must_revalidate: bool,
    data: Option<Bytes>,
    modified: Option<i64>,
    expires: Option<i64>,
    etag: Option<String>,
}

fn read_response(r: &mut Reader<'_>) -> Option<ResponseDump> {
    if r.u8() == 0 {
        return None;
    }
    let error = if r.u8() != 0 {
        let reason = r.u8();
        Some((reason, r.string()))
    } else {
        None
    };
    let no_content = r.u8() != 0;
    let not_modified = r.u8() != 0;
    let must_revalidate = r.u8() != 0;
    let data = if r.u8() != 0 { Some(Bytes(r.bytes())) } else { None };
    let modified = if r.u8() != 0 { Some(r.i64()) } else { None };
    let expires = if r.u8() != 0 { Some(r.i64()) } else { None };
    let etag = if r.u8() != 0 { Some(r.string()) } else { None };
    Some(ResponseDump { error, no_content, not_modified, must_revalidate, data, modified, expires, etag })
}

fn reason_code(reason: Reason) -> u8 {
    reason as u8
}

fn dump(response: &Response) -> ResponseDump {
    ResponseDump {
        error: response.error.as_ref().map(|e| (reason_code(e.reason), e.message.clone())),
        no_content: response.no_content,
        not_modified: response.not_modified,
        must_revalidate: response.must_revalidate,
        data: response.data.as_ref().map(|d| Bytes(d.to_vec())),
        modified: response.modified.map(|t| t.secs_since_epoch()),
        expires: response.expires.map(|t| t.secs_since_epoch()),
        etag: response.etag.clone(),
    }
}

type LogRecord = (u8, u8, i64, String);

fn read_records(r: &mut Reader<'_>) -> Vec<LogRecord> {
    let n = r.u32();
    (0..n)
        .map(|_| {
            let severity = r.u8();
            let event = r.u8();
            let code = r.i64();
            (severity, event, code, r.string())
        })
        .collect()
}

/// Captures the Rust log records of the calling thread.
struct Capture(Arc<Mutex<Vec<LogRecord>>>);

impl Observer for Capture {
    fn on_record(&self, severity: EventSeverity, event: Event, code: i64, msg: &str) -> bool {
        self.0.lock().unwrap().push((severity as u8, event as u8, code, msg.to_owned()));
        true
    }
}

fn capture_rust_logs<T>(f: impl FnOnce() -> T) -> (T, Vec<LogRecord>) {
    let records = Arc::new(Mutex::new(Vec::new()));
    let previous = Log::set_thread_observer(Some(Box::new(Capture(records.clone()))));
    let result = f();
    Log::set_thread_observer(previous);
    let records = records.lock().unwrap().clone();
    (result, records)
}

// ---- databases ----

fn cpp_path(relative: &str) -> PathBuf {
    let base = std::env::var("TEST_SRCDIR").or_else(|_| std::env::var("RUNFILES_DIR")).expect("TEST_SRCDIR");
    PathBuf::from(base).join("maplibre+").join(relative)
}

fn copy_to_tmp(relative: &str, tag: &str) -> String {
    let dir = std::env::var("TEST_TMPDIR").expect("TEST_TMPDIR");
    let name = relative.rsplit('/').next().unwrap();
    let path = format!("{dir}/{tag}-{name}");
    std::fs::copy(cpp_path(relative), &path).unwrap();
    path
}

const DATABASES: [&str; 2] = ["metrics/cache-style.db", "metrics/cache-metrics.db"];

/// One resource per row of `resources` and `tiles`, plus edge cases.
fn resources_of(path: &str) -> Vec<Resource> {
    let conn = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut resources = Vec::new();

    let kinds = [
        Kind::Unknown,
        Kind::Style,
        Kind::Source,
        Kind::Tile,
        Kind::Glyphs,
        Kind::SpriteImage,
        Kind::SpriteJSON,
        Kind::Image,
    ];
    let mut stmt = conn.prepare("SELECT url, kind FROM resources ORDER BY id").unwrap();
    let rows: Vec<(String, i64)> = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    for (url, kind) in &rows {
        // Kind::Tile in the resources table would take the tile path; use
        // Unknown for such rows (the key is the URL either way).
        let kind = kinds.get(*kind as usize).copied().filter(|k| *k != Kind::Tile).unwrap_or(Kind::Unknown);
        resources.push(Resource::with_tile_data(kind, url.clone(), None, LoadingMethod::CacheOnly));
        // The same key under another kind, a data range (another cache key)
        // and a volatile resource.
        resources.push(Resource::with_tile_data(Kind::Unknown, url.clone(), None, LoadingMethod::CacheOnly));
        let mut ranged = Resource::with_tile_data(kind, url.clone(), None, LoadingMethod::CacheOnly);
        ranged.data_range = Some((0, 99));
        resources.push(ranged);
        let mut volatile = Resource::with_tile_data(kind, url.clone(), None, LoadingMethod::CacheOnly);
        volatile.storage_policy = StoragePolicy::Volatile;
        resources.push(volatile);
    }

    let mut stmt = conn.prepare("SELECT url_template, pixel_ratio, z, x, y FROM tiles ORDER BY id").unwrap();
    let tiles: Vec<TileData> = stmt
        .query_map([], |row| {
            Ok(TileData {
                url_template: row.get(0)?,
                pixel_ratio: row.get::<_, i64>(1)? as u8,
                z: row.get::<_, i64>(2)? as i8,
                x: row.get::<_, i64>(3)? as i32,
                y: row.get::<_, i64>(4)? as i32,
            })
        })
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    for tile in &tiles {
        let resource = |t: TileData| {
            Resource::with_tile_data(Kind::Tile, t.url_template.clone(), Some(t), LoadingMethod::CacheOnly)
        };
        resources.push(resource(tile.clone()));
        // Missing neighbours: other pixel ratio, x + 1.
        resources.push(resource(TileData { pixel_ratio: tile.pixel_ratio.wrapping_add(1), ..tile.clone() }));
        resources.push(resource(TileData { x: tile.x + 1, ..tile.clone() }));
    }

    // Keys that are not there.
    resources.push(Resource::with_tile_data(Kind::Style, "http://missing/style.json", None, LoadingMethod::CacheOnly));
    resources.push(Resource::with_tile_data(Kind::SpriteJSON, "", None, LoadingMethod::CacheOnly));
    resources.push(Resource::with_tile_data(
        Kind::Tile,
        "http://missing/{z}/{x}/{y}.png",
        Some(TileData { url_template: "http://missing/{z}/{x}/{y}.png".to_owned(), pixel_ratio: 1, x: 0, y: 0, z: 0 }),
        LoadingMethod::CacheOnly,
    ));
    resources
}

#[test]
fn offline_database_get_matches_cpp_for_every_row() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut cases = 0;
    let mut found = 0;
    for relative in DATABASES {
        let path = copy_to_tmp(relative, "offline-db");
        let resources = resources_of(&path);
        // The OfflineDatabase does not look at volatility.
        let resources: Vec<Resource> =
            resources.into_iter().filter(|r| r.storage_policy != StoragePolicy::Volatile).collect();

        let mut request = Writer::default();
        request.bytes(path.as_bytes());
        request.0.extend_from_slice(&encode_resources(&resources));
        let output = oracle_call(1, &request.0);
        let mut reader = Reader(&output);
        let count = reader.u32() as usize;
        assert_eq!(resources.len(), count);
        let cpp: Vec<Option<ResponseDump>> = (0..count).map(|_| read_response(&mut reader)).collect();
        let cpp_logs = read_records(&mut reader);

        let mut db = OfflineDatabase::new(path.clone(), &TileServerOptions::default());
        db.reopen_database_read_only(true);
        let (rust, rust_logs) =
            capture_rust_logs(|| resources.iter().map(|r| db.get(r).map(|res| dump(&res))).collect::<Vec<_>>());

        for (i, resource) in resources.iter().enumerate() {
            assert_eq!(cpp[i], rust[i], "{relative}: {resource:?}");
            if cpp[i].is_some() {
                found += 1;
            }
        }
        assert_eq!(cpp_logs, rust_logs, "{relative}: log records");
        cases += resources.len();
    }
    eprintln!("offline database: {cases} lookups identical ({found} found)");
    assert!(found > 600);
}

fn rust_dbfs_request_all(path: &str, resources: &[Resource]) -> Vec<Option<ResponseDump>> {
    let run_loop = RunLoop::new();
    let dbfs = DatabaseFileSource::new(&ResourceOptions::default().with_cache_path(path), &ClientOptions::default());
    dbfs.set_property(READ_ONLY_MODE_KEY, &feature::Value::Bool(true));
    let responses: Rc<RefCell<Vec<Option<ResponseDump>>>> = Rc::new(RefCell::new(vec![None; resources.len()]));
    let received = Rc::new(RefCell::new(0usize));
    let count = resources.len();
    let mut requests = Vec::new();
    for (i, resource) in resources.iter().enumerate() {
        let (responses, received, stop) = (responses.clone(), received.clone(), run_loop.clone());
        requests.push(dbfs.request(
            resource,
            Box::new(move |res: Response| {
                responses.borrow_mut()[i] = Some(dump(&res));
                *received.borrow_mut() += 1;
                if *received.borrow() == count {
                    stop.stop();
                }
            }),
        ));
    }
    if count > 0 {
        run_loop.run();
    }
    drop(requests);
    let result = responses.borrow().clone();
    result
}

#[test]
fn database_file_source_matches_cpp_for_every_row() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut cases = 0;
    for relative in DATABASES {
        let path = copy_to_tmp(relative, "dbfs");
        let resources = resources_of(&path);

        let mut request = Writer::default();
        request.bytes(path.as_bytes());
        request.0.extend_from_slice(&encode_resources(&resources));
        let output = oracle_call(2, &request.0);
        let mut reader = Reader(&output);
        let count = reader.u32() as usize;
        assert_eq!(resources.len(), count);
        let cpp: Vec<Option<ResponseDump>> = (0..count).map(|_| read_response(&mut reader)).collect();

        let rust = rust_dbfs_request_all(&path, &resources);
        for (i, resource) in resources.iter().enumerate() {
            assert!(cpp[i].is_some(), "{relative}: C++ never answered {resource:?}");
            assert_eq!(cpp[i], rust[i], "{relative}: {resource:?}");
        }
        cases += resources.len();
    }
    eprintln!("database file source: {cases} requests identical");
}

// ---- sprites ----

#[derive(Debug, PartialEq)]
struct ImageDump {
    id: String,
    width: u32,
    height: u32,
    pixel_ratio: u32,
    sdf: bool,
    stretch_x: Vec<(u32, u32)>,
    stretch_y: Vec<(u32, u32)>,
    content: Option<[u32; 4]>,
    text_fit_width: u8,
    text_fit_height: u8,
    pixels: Bytes,
}

#[derive(Debug)]
struct SpriteResult {
    result: Result<Vec<ImageDump>, String>,
    logs: Vec<LogRecord>,
}

fn cpp_parse_sprite(id: &str, image: &[u8], json: &[u8]) -> SpriteResult {
    let mut request = Writer::default();
    request.bytes(id.as_bytes());
    request.bytes(image);
    request.bytes(json);
    let output = oracle_call(3, &request.0);
    let mut r = Reader(&output);
    let result = if r.u8() != 0 {
        let n = r.u32();
        Ok((0..n)
            .map(|_| {
                let id = r.string();
                let width = r.u32();
                let height = r.u32();
                let pixel_ratio = r.f32().to_bits();
                let sdf = r.u8() != 0;
                let mut stretches = || {
                    let n = r.u32();
                    (0..n).map(|_| (r.f32().to_bits(), r.f32().to_bits())).collect::<Vec<_>>()
                };
                let stretch_x = stretches();
                let stretch_y = stretches();
                let content = if r.u8() != 0 {
                    Some([r.f32().to_bits(), r.f32().to_bits(), r.f32().to_bits(), r.f32().to_bits()])
                } else {
                    None
                };
                let text_fit_width = r.u8();
                let text_fit_height = r.u8();
                let pixels = Bytes(r.bytes());
                ImageDump {
                    id,
                    width,
                    height,
                    pixel_ratio,
                    sdf,
                    stretch_x,
                    stretch_y,
                    content,
                    text_fit_width,
                    text_fit_height,
                    pixels,
                }
            })
            .collect())
    } else {
        Err(r.string())
    };
    let logs = read_records(&mut r);
    SpriteResult { result, logs }
}

fn rust_parse_sprite(id: &str, image: &[u8], json: &[u8]) -> SpriteResult {
    let (result, logs) = capture_rust_logs(|| parse_sprite(id, image, json));
    let result = result
        .map(|images| {
            images
                .iter()
                .map(|img| ImageDump {
                    id: img.id.clone(),
                    width: img.image.size.width,
                    height: img.image.size.height,
                    pixel_ratio: img.pixel_ratio.to_bits(),
                    sdf: img.sdf,
                    stretch_x: img.stretch_x.iter().map(|s| (s.0.to_bits(), s.1.to_bits())).collect(),
                    stretch_y: img.stretch_y.iter().map(|s| (s.0.to_bits(), s.1.to_bits())).collect(),
                    content: img
                        .content
                        .map(|c| [c.left.to_bits(), c.top.to_bits(), c.right.to_bits(), c.bottom.to_bits()]),
                    text_fit_width: img.text_fit_width.map_or(0, |t| t as u8 + 1),
                    text_fit_height: img.text_fit_height.map_or(0, |t| t as u8 + 1),
                    pixels: Bytes(img.image.data.clone()),
                })
                .collect()
        })
        .map_err(|e| e.to_string());
    SpriteResult { result, logs }
}

const JSON_ERROR_PREFIX: &str = "Failed to parse JSON: ";

/// Compares one parse; returns the number of images.
fn compare_sprite(case: &str, id: &str, image: &[u8], json: &[u8], image_is_valid: bool) -> usize {
    let cpp = cpp_parse_sprite(id, image, json);
    let rust = rust_parse_sprite(id, image, json);
    match (&cpp.result, &rust.result) {
        (Ok(a), Ok(b)) => {
            assert_eq!(a.len(), b.len(), "{case}: image count");
            for (x, y) in a.iter().zip(b) {
                assert_eq!(x, y, "{case}: image {}", x.id);
            }
            assert_eq!(cpp.logs, rust.logs, "{case}: log records");
            a.len()
        }
        (Err(a), Err(b)) => {
            if a.starts_with(JSON_ERROR_PREFIX) || !image_is_valid {
                assert_eq!(a.starts_with(JSON_ERROR_PREFIX), b.starts_with(JSON_ERROR_PREFIX), "{case}: {a} vs {b}");
            } else {
                assert_eq!(a, b, "{case}");
                assert_eq!(cpp.logs, rust.logs, "{case}: log records");
            }
            0
        }
        _ => panic!(
            "{case}: C++ {:?} vs Rust {:?}",
            cpp.result.as_ref().map(|v| v.len()),
            rust.result.as_ref().map(|v| v.len())
        ),
    }
}

fn read(relative: &str) -> Vec<u8> {
    std::fs::read(cpp_path(relative)).unwrap()
}

/// The sprite pairs stored in a cache database (`<base>.json` +
/// `<base>.png`), decompressed through the Rust OfflineDatabase (checked
/// against C++ above).
fn database_sprites(relative: &str) -> Vec<(String, Vec<u8>, Vec<u8>)> {
    let path = copy_to_tmp(relative, "sprites");
    let mut db = OfflineDatabase::new(path.clone(), &TileServerOptions::default());
    db.reopen_database_read_only(true);
    let conn = rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut stmt = conn.prepare("SELECT url FROM resources WHERE kind = 6 ORDER BY id").unwrap();
    let urls: Vec<String> = stmt.query_map([], |row| row.get(0)).unwrap().map(|r| r.unwrap()).collect();
    let mut sprites = Vec::new();
    for json_url in urls {
        let Some(base) = json_url.strip_suffix(".json") else {
            continue;
        };
        let get = |db: &mut OfflineDatabase, url: String| {
            db.get(&Resource::with_tile_data(Kind::Unknown, url, None, LoadingMethod::CacheOnly))
                .and_then(|r| r.data)
                .map(|d| d.to_vec())
        };
        let (Some(json), Some(image)) = (get(&mut db, json_url.clone()), get(&mut db, format!("{base}.png"))) else {
            continue;
        };
        sprites.push((json_url, image, json));
    }
    sprites
}

#[test]
fn parse_sprite_matches_cpp_on_fixtures() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut cases = 0;
    let mut images = 0;
    let fixtures = [
        ("test/fixtures/annotations/emerald.png", "test/fixtures/annotations/emerald.json"),
        ("test/fixtures/annotations/emerald@2x.png", "test/fixtures/annotations/emerald@2x.json"),
        ("test/fixtures/resources/sprite.png", "test/fixtures/resources/sprite.json"),
        (
            "test/fixtures/resources/versatiles-sprite/sprite.png",
            "test/fixtures/resources/versatiles-sprite/sprite.json",
        ),
        (
            "test/fixtures/resources/versatiles-sprite/sprite@2x.png",
            "test/fixtures/resources/versatiles-sprite/sprite@2x.json",
        ),
    ];
    for (image, json) in fixtures {
        let (image, json) = (read(image), read(json));
        for id in ["default", "custom"] {
            images += compare_sprite(
                &format!("{id} {}", String::from_utf8_lossy(&json[..json.len().min(40)])),
                id,
                &image,
                &json,
                true,
            );
            cases += 1;
        }
    }
    for relative in DATABASES {
        for (url, image, json) in database_sprites(relative) {
            images += compare_sprite(&format!("{relative} {url}"), "default", &image, &json, true);
            cases += 1;
        }
    }
    // Corrupt inputs.
    let emerald = read("test/fixtures/annotations/emerald.png");
    for json in [&b"CORRUPT"[..], b"", b"[]", b"42", b"{ \"image\": \" }", b"{}"] {
        compare_sprite(&format!("json {:?}", String::from_utf8_lossy(json)), "default", &emerald, json, true);
        cases += 1;
    }
    for image in [&b"CORRUPT"[..], b""] {
        compare_sprite("corrupt image", "default", image, b"{}", false);
        cases += 1;
    }
    eprintln!("parseSprite fixtures: {cases} cases identical ({images} images)");
    assert!(images > 1000);
}

// ---- generated sprite JSON ----

fn number() -> impl Strategy<Value = String> {
    prop_oneof![
        (-5i64..300).prop_map(|v| v.to_string()),
        (-4i64..4000).prop_map(|v| format!("{}", v as f64 / 4.0)),
        Just("65535".to_owned()),
        Just("65536".to_owned()),
        Just("1e2".to_owned()),
        Just("0.0".to_owned()),
        Just("-0.5".to_owned()),
        Just("1024".to_owned()),
        Just("1025".to_owned()),
    ]
}

fn value() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => number(),
        1 => Just("true".to_owned()),
        1 => Just("false".to_owned()),
        1 => Just("null".to_owned()),
        1 => Just("\"12\"".to_owned()),
        1 => Just("[]".to_owned()),
        1 => Just("{}".to_owned()),
    ]
}

fn stretches() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => prop::collection::vec((number(), number()), 0..3)
            .prop_map(|v| format!("[{}]", v.iter().map(|(a, b)| format!("[{a}, {b}]")).collect::<Vec<_>>().join(", "))),
        1 => value(),
        1 => Just("[[1, 2, 3]]".to_owned()),
        1 => Just("[[4, false]]".to_owned()),
        1 => Just("[5]".to_owned()),
    ]
}

fn content() -> impl Strategy<Value = String> {
    prop_oneof![
        3 => (number(), number(), number(), number()).prop_map(|(a, b, c, d)| format!("[{a}, {b}, {c}, {d}]")),
        1 => value(),
        1 => Just("[1, 2, 3]".to_owned()),
        1 => Just("[1, 2, 3, \"4\"]".to_owned()),
    ]
}

fn text_fit() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("\"stretchOrShrink\"".to_owned()),
        Just("\"stretchOnly\"".to_owned()),
        Just("\"proportional\"".to_owned()),
        Just("\"invalid\"".to_owned()),
        value(),
    ]
}

fn entry() -> impl Strategy<Value = String> {
    let field = |name: &'static str, s: BoxedStrategy<String>| {
        prop::option::weighted(0.7, s).prop_map(move |v| v.map(|v| format!("\"{name}\": {v}")))
    };
    let small = || {
        prop_oneof![
            4 => (0i64..120).prop_map(|v| v.to_string()),
            1 => number(),
            1 => value(),
        ]
        .boxed()
    };
    (
        (field("x", small()), field("y", small()), field("width", small()), field("height", small())),
        (
            field("pixelRatio", prop_oneof![3 => Just("1".to_owned()), 1 => number(), 1 => value()].boxed()),
            field("sdf", prop_oneof![Just("true".to_owned()), Just("false".to_owned()), value()].boxed()),
            field("stretchX", stretches().boxed()),
            field("stretchY", stretches().boxed()),
            field("content", content().boxed()),
            field("textFitWidth", text_fit().boxed()),
            field("textFitHeight", text_fit().boxed()),
        ),
    )
        .prop_map(|((x, y, w, h), (pr, sdf, sx, sy, c, tw, th))| {
            let fields: Vec<String> = [x, y, w, h, pr, sdf, sx, sy, c, tw, th].into_iter().flatten().collect();
            format!("{{{}}}", fields.join(", "))
        })
}

fn sprite_json() -> impl Strategy<Value = String> {
    prop::collection::vec(prop_oneof![9 => entry(), 1 => value()], 0..6).prop_map(|entries| {
        let members: Vec<String> = entries.iter().enumerate().map(|(i, e)| format!("\"image{i}\": {e}")).collect();
        format!("{{{}}}", members.join(", "))
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn parse_sprite_matches_cpp_on_generated_json(json in sprite_json(), id in prop_oneof![Just("default"), Just("icons")], two_x in any::<bool>()) {
        let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        thread_local! {
            static IMAGES: (Vec<u8>, Vec<u8>) = (
                read("test/fixtures/annotations/emerald.png"),
                read("test/fixtures/annotations/emerald@2x.png"),
            );
        }
        IMAGES.with(|(one, two)| {
            let image = if two_x { two } else { one };
            compare_sprite(&json, id, image, json.as_bytes(), true);
        });
    }
}
