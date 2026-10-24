//! ScreenScraper API v2 (ADR-0035, research/03 §2.4, research/14 §4): all
//! eight picture types, tagged with the market they show, looked up by
//! the game's serial.
//!
//! - The API answers only to a registered program: `devid`, `devpassword`
//!   and `softname` come from the environment or are built into a release
//!   (`YAOPLM_SCREENSCRAPER_*`, [`dev_credentials`]); without them the
//!   source is off. A user account (`ssid`, `sspassword`) is optional and
//!   raises the user's quota. None of these is ever logged or stored: the
//!   URLs in the log and the cache are [`redact`]ed.
//! - One `jeuInfos.php` request per game, by `serialnum` on system 58
//!   (PS2), cached for 30 days; the XML lists every media with its type
//!   and region. The media are fetched from the URLs it gives.
//! - Media types: `box-2D` (COV), `box-2D-back` (COV2), `box-2D-side`
//!   (LAB), `support-2D` (ICO), `ss` (SCR), `sstitle` (SCR2), `fanart`
//!   (BG), `wheel-hd` and `wheel` (LGO). Names and the XML layout follow
//!   the API documentation (`screenscraper.fr/webapi2.php`) and ES-DE's
//!   client (`es-app/src/scrapers/ScreenScraper.{h,cpp}`), which reads
//!   `Data/jeu/medias/media` with `type`, `region` and `format` attributes.
//! - Packaging only from media of the game's market; screenshots, title
//!   screens, backgrounds and logos from any, the game's market first.
//! - Errors pause the source: 401 (busy) for ten minutes, a used-up daily
//!   quota (430, 431) for six hours, refused credentials (403), a blocked
//!   version (426) or a closed API (423) for the rest of the session.
//! - Content is CC BY-NC-SA 4.0: credited on the About screen, kept in the
//!   user's cache, never bundled.

use std::sync::Mutex;
use std::time::Duration;

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};
use tokio::time::Instant;
use yaoplm_formats::art::ArtKind;

use crate::NetError;
use crate::region::{self, Family};

pub const LIMITER: &str = "screenscraper";
pub const API_BASE: &str = "https://api.screenscraper.fr/api2";
/// ScreenScraper's system id of the PlayStation 2.
pub const SYSTEM_PS2: u32 = 58;
/// How long a game's media list is believed.
pub const INFO_TTL: Duration = Duration::from_secs(30 * 24 * 3600);
const MAX_VARIANTS: usize = 3;

/// The program's registration with ScreenScraper.
#[derive(Clone, PartialEq, Eq)]
pub struct DevCredentials {
    pub devid: String,
    pub devpassword: String,
    pub softname: String,
}

impl std::fmt::Debug for DevCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DevCredentials")
            .field("softname", &self.softname)
            .finish_non_exhaustive()
    }
}

/// A user's ScreenScraper account.
#[derive(Clone, PartialEq, Eq)]
pub struct Account {
    pub user: String,
    pub password: String,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// The developer credentials: from the environment at run time, or the
/// ones a release was built with (CI secrets, never in the repository).
pub fn dev_credentials() -> Option<DevCredentials> {
    let built = (
        option_env!("YAOPLM_SCREENSCRAPER_DEVID"),
        option_env!("YAOPLM_SCREENSCRAPER_DEVPASSWORD"),
    );
    let devid = env("YAOPLM_SCREENSCRAPER_DEVID").or(built.0.map(str::to_owned))?;
    let devpassword = env("YAOPLM_SCREENSCRAPER_DEVPASSWORD").or(built.1.map(str::to_owned))?;
    let softname = env("YAOPLM_SCREENSCRAPER_SOFTNAME")
        .or(option_env!("YAOPLM_SCREENSCRAPER_SOFTNAME").map(str::to_owned))
        .unwrap_or_else(|| format!("YAOPLM {}", env!("CARGO_PKG_VERSION")));
    (!devid.is_empty() && !devpassword.is_empty()).then_some(DevCredentials {
        devid,
        devpassword,
        softname,
    })
}

/// A user account from `YAOPLM_SCREENSCRAPER_USER` and `_PASSWORD`.
pub fn account_from_env() -> Option<Account> {
    Some(Account {
        user: env("YAOPLM_SCREENSCRAPER_USER")?,
        password: env("YAOPLM_SCREENSCRAPER_PASSWORD")?,
    })
}

/// Where the API is and who asks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Api {
    pub base: String,
    pub dev: Option<DevCredentials>,
}

impl Default for Api {
    fn default() -> Self {
        Self {
            base: API_BASE.to_owned(),
            dev: dev_credentials(),
        }
    }
}

const SECRETS: [&str; 4] = ["devid", "devpassword", "ssid", "sspassword"];

/// `url` with the values of credential parameters replaced by `***`.
pub fn redact(url: &str) -> String {
    redact_text(url)
}

/// [`redact`] for any text that may hold such URLs, such as an error.
pub fn redact_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(['?', '&']) {
        out.push_str(&rest[..=at]);
        rest = &rest[at + 1..];
        let key_end = rest.find('=').unwrap_or(rest.len());
        let key = &rest[..key_end];
        if key_end < rest.len() && SECRETS.contains(&key.to_ascii_lowercase().as_str()) {
            let value_end = rest[key_end + 1..]
                .find(|c: char| c == '&' || c.is_whitespace() || matches!(c, '"' | '\'' | ')'))
                .map_or(rest.len(), |end| key_end + 1 + end);
            out.push_str(key);
            out.push_str("=***");
            rest = &rest[value_end..];
        }
    }
    out.push_str(rest);
    out
}

/// Everything but unreserved characters is percent-encoded.
const QUERY: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

fn encode(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, QUERY).to_string()
}

impl Api {
    /// The `jeuInfos.php` request for a serial (`SLUS-20071`).
    pub fn info_url(&self, account: Option<&Account>, serial: &str) -> Option<String> {
        let dev = self.dev.as_ref()?;
        let mut url = format!(
            "{}/jeuInfos.php?devid={}&devpassword={}&softname={}&output=xml",
            self.base,
            encode(&dev.devid),
            encode(&dev.devpassword),
            encode(&dev.softname)
        );
        if let Some(account) = account {
            url.push_str(&format!(
                "&ssid={}&sspassword={}",
                encode(&account.user),
                encode(&account.password)
            ));
        }
        url.push_str(&format!(
            "&systemeid={SYSTEM_PS2}&romtype=iso&romnom={}&serialnum={}",
            encode(&format!("{serial}.iso")),
            encode(serial)
        ));
        Some(url)
    }
}

/// One media of a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Media {
    /// `box-2D`, `ss`...
    pub kind: String,
    /// `us`, `eu`, `jp`, `wor`...
    pub region: String,
    pub format: Option<String>,
    pub url: String,
}

/// What `jeuInfos.php` says about a game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GameInfo {
    pub id: Option<String>,
    pub medias: Vec<Media>,
    pub requests_today: Option<u64>,
    pub max_requests_per_day: Option<u64>,
}

impl GameInfo {
    /// The user's daily quota is used up.
    pub fn quota_spent(&self) -> bool {
        matches!((self.requests_today, self.max_requests_per_day), (Some(done), Some(max)) if max > 0 && done >= max)
    }
}

fn attr(element: &BytesStart<'_>, name: &str) -> Option<String> {
    element
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == name)
        .and_then(|a| {
            a.normalized_value(XmlVersion::Implicit1_0)
                .ok()
                .map(|v| v.into_owned())
        })
}

/// Reads the XML answer of `jeuInfos.php`.
pub fn parse_game_info(xml: &[u8]) -> Result<GameInfo, NetError> {
    let mut reader = Reader::from_reader(xml);
    let mut info = GameInfo::default();
    let mut path: Vec<String> = Vec::new();
    let mut media: Option<Media> = None;
    let mut text = String::new();
    let mut buf = Vec::new();
    let mut saw_data = false;
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| NetError::Http(format!("ScreenScraper: {e}")))?;
        match event {
            Event::Start(start) => {
                let name = start.local_name().as_ref().to_owned();
                saw_data |= name == "Data";
                if name == "media" && path.last().map(String::as_str) == Some("medias") {
                    media = Some(Media {
                        kind: attr(&start, "type").unwrap_or_default(),
                        region: attr(&start, "region")
                            .unwrap_or_default()
                            .to_ascii_lowercase(),
                        format: attr(&start, "format"),
                        url: String::new(),
                    });
                } else if name == "jeu" {
                    info.id = attr(&start, "id");
                }
                path.push(name);
                text.clear();
            }
            Event::Text(t) => text.push_str(&t.xml10_content()),
            Event::CData(t) => text.push_str(&t),
            Event::GeneralRef(r) => match r.resolve_char_ref() {
                Ok(Some(c)) => text.push(c),
                _ => text.push_str(resolve_predefined_entity(&r).unwrap_or("")),
            },
            Event::End(_) => {
                let parent = path.iter().rev().nth(1).map(String::as_str);
                let value = std::mem::take(&mut text);
                match (parent, path.last().map(String::as_str)) {
                    (Some("medias"), Some("media")) => {
                        if let Some(done) = media.take()
                            && !done.kind.is_empty()
                            && !value.trim().is_empty()
                        {
                            info.medias.push(Media {
                                url: value.trim().replace(' ', "%20"),
                                ..done
                            });
                        }
                    }
                    (Some("ssuser"), Some("requeststoday")) => {
                        info.requests_today = value.trim().parse().ok();
                    }
                    (Some("ssuser"), Some("maxrequestsperday")) => {
                        info.max_requests_per_day = value.trim().parse().ok();
                    }
                    _ => {}
                }
                path.pop();
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if !saw_data {
        return Err(NetError::Http("ScreenScraper: not a game answer".into()));
    }
    Ok(info)
}

/// The media types of a slot, best first.
pub fn media_types(kind: ArtKind) -> &'static [&'static str] {
    match kind {
        ArtKind::Cov => &["box-2D"],
        ArtKind::Cov2 => &["box-2D-back"],
        ArtKind::Lab => &["box-2D-side"],
        ArtKind::Ico => &["support-2D"],
        ArtKind::Scr => &["ss"],
        ArtKind::Scr2 => &["sstitle"],
        ArtKind::Bg => &["fanart"],
        ArtKind::Lgo => &["wheel-hd", "wheel"],
    }
}

/// ScreenScraper's region codes of a market, the main one first.
fn family_regions(family: Family) -> &'static [&'static str] {
    match family {
        Family::America => &["us", "ca", "ame", "br"],
        Family::Europe => &[
            "eu", "uk", "fr", "de", "sp", "es", "it", "nl", "se", "dk", "fi", "no", "pt", "au",
            "nz", "pl", "gr", "ru", "be", "ch", "at", "sca",
        ],
        Family::Asia => &["jp", "kr", "asi", "cn", "tw", "hk"],
    }
}

/// The region code of the serial's own market: `kr` for a Korean
/// release, `asi` for an Asian one, otherwise the family's main code.
fn own_region(serial: &str) -> Option<&'static str> {
    let prefix = serial.get(..4)?.to_ascii_uppercase();
    match prefix.as_str() {
        "SLKA" | "SCKA" => Some("kr"),
        "SCAJ" | "SLAJ" => Some("asi"),
        _ => region::of_id(serial).map(|f| family_regions(f)[0]),
    }
}

/// The media to try for a slot, best first. Packaging comes only from the
/// game's market (a serial with no market takes any); other types from
/// any market, the game's own first, then `wor` and `ss`.
pub fn pick<'a>(
    medias: &'a [Media],
    kind: ArtKind,
    serial: &str,
    variants: bool,
) -> Vec<&'a Media> {
    let types = media_types(kind);
    let family = region::of_id(serial);
    let own = own_region(serial);
    let rank = |region: &str| -> Option<usize> {
        let in_family = family.and_then(|f| family_regions(f).iter().position(|r| *r == region));
        match (region::region_specific(kind), family) {
            (_, _) if Some(region) == own => Some(0),
            (_, Some(_)) if in_family.is_some() => in_family.map(|p| 1 + p),
            (true, Some(_)) => None,
            _ => Some(match region {
                "wor" => 100,
                "ss" => 101,
                _ => 102,
            }),
        }
    };
    let mut found: Vec<(usize, usize, &Media)> = medias
        .iter()
        .filter_map(|m| {
            let by_type = types.iter().position(|t| *t == m.kind)?;
            Some((rank(&m.region)?, by_type, m))
        })
        .collect();
    found.sort_by_key(|(region, by_type, _)| (*region, *by_type));
    found.dedup_by(|a, b| a.2.url == b.2.url);
    found.truncate(if variants { MAX_VARIANTS } else { usize::MAX });
    found.into_iter().map(|(_, _, m)| m).collect()
}

/// What an HTTP status of the API means, and how long to leave it alone.
pub fn status_pause(status: u16) -> (String, Option<Duration>) {
    const SESSION: Duration = Duration::from_secs(24 * 3600);
    match status {
        401 => (
            "ScreenScraper is closed to non-members while it is busy (401)".into(),
            Some(Duration::from_secs(600)),
        ),
        403 => (
            "ScreenScraper refused this build's developer credentials (403)".into(),
            Some(SESSION),
        ),
        423 => ("ScreenScraper's API is closed (423)".into(), Some(SESSION)),
        426 => (
            "ScreenScraper no longer accepts this version (426): update YAOPLM".into(),
            Some(SESSION),
        ),
        430 => (
            "the daily ScreenScraper quota is used up (430)".into(),
            Some(Duration::from_secs(6 * 3600)),
        ),
        431 => (
            "ScreenScraper was asked about too many unknown games today (431)".into(),
            Some(Duration::from_secs(6 * 3600)),
        ),
        other => (format!("ScreenScraper answered HTTP {other}"), None),
    }
}

/// The client: the API and a pause after errors that would only repeat.
#[derive(Debug)]
pub struct ScreenScraper {
    pub api: Api,
    paused: Mutex<Option<(Instant, String)>>,
}

impl ScreenScraper {
    pub fn new(api: Api) -> Self {
        Self {
            api,
            paused: Mutex::new(None),
        }
    }

    /// Why the source is resting, if it is.
    pub fn paused(&self) -> Option<String> {
        let mut paused = self
            .paused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match paused.as_ref() {
            Some((until, why)) if Instant::now() < *until => Some(why.clone()),
            Some(_) => {
                *paused = None;
                None
            }
            None => None,
        }
    }

    pub fn pause(&self, why: &str, how_long: Duration) {
        *self
            .paused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((Instant::now() + how_long, why.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const DOA2_JP: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Data>
  <header><APIversion>2.0</APIversion><success>true</success></header>
  <ssuser><id>someone</id><niveau>1</niveau><requeststoday>12</requeststoday><maxrequestsperday>20000</maxrequestsperday></ssuser>
  <jeu id="3940" romid="1">
    <noms><nom region="jp">Dead or Alive 2</nom></noms>
    <medias>
      <media type="box-2D" parent="jeu" region="eu" format="png">https://neoclone.screenscraper.fr/api2/mediaJeu.php?devid=d&amp;devpassword=p&amp;softname=s&amp;ssid=&amp;sspassword=&amp;systemeid=58&amp;jeuid=3940&amp;media=box-2D(eu)</media>
      <media type="box-2D" parent="jeu" region="jp" format="png">https://neoclone.screenscraper.fr/api2/mediaJeu.php?jeuid=3940&amp;media=box-2D(jp)</media>
      <media type="box-2D-side" parent="jeu" region="wor" format="png">https://x/side(wor)</media>
      <media type="support-2D" parent="jeu" region="JP" format="png">https://x/disc(jp)</media>
      <media type="ss" parent="jeu" region="wor" format="png">https://x/ss(wor)</media>
      <media type="ss" parent="jeu" region="eu" format="png">https://x/ss(eu)</media>
      <media type="wheel" parent="jeu" region="jp" format="png">https://x/wheel(jp)</media>
      <media type="wheel-hd" parent="jeu" region="jp" format="png">https://x/wheel-hd(jp)</media>
      <media type="fanart" parent="jeu" format="jpg"><![CDATA[https://x/fan art]]></media>
    </medias>
  </jeu>
</Data>"#;

    #[test]
    fn reads_the_media_list() {
        let info = parse_game_info(DOA2_JP.as_bytes()).unwrap();
        assert_eq!(info.id.as_deref(), Some("3940"));
        assert_eq!(info.medias.len(), 9);
        assert_eq!(info.medias[0].kind, "box-2D");
        assert_eq!(info.medias[0].region, "eu");
        assert!(info.medias[0].url.contains("&devpassword=p&"));
        assert_eq!(info.medias[3].region, "jp", "regions are lower-cased");
        assert_eq!(info.medias[8].url, "https://x/fan%20art");
        assert_eq!(info.medias[8].region, "");
        assert_eq!(
            (info.requests_today, info.max_requests_per_day),
            (Some(12), Some(20000))
        );
        assert!(!info.quota_spent());
        assert!(parse_game_info(b"Erreur : Rom/Iso/Dossier non trouvee !").is_err());
        assert!(parse_game_info(b"<html><body>busy</body></html>").is_err());
    }

    #[test]
    fn packaging_only_from_the_games_market() {
        let info = parse_game_info(DOA2_JP.as_bytes()).unwrap();
        let urls = |kind, serial, variants| -> Vec<String> {
            pick(&info.medias, kind, serial, variants)
                .iter()
                .map(|m| m.url.clone())
                .collect()
        };
        // The Japanese box for the Japanese disc; the PAL one never.
        let jp = urls(ArtKind::Cov, "SLPS-25002", true);
        assert_eq!(jp.len(), 1);
        assert!(jp[0].ends_with("box-2D(jp)"));
        assert!(urls(ArtKind::Cov, "SCES-50003", false)[0].ends_with("box-2D(eu)"));
        assert!(urls(ArtKind::Cov, "SLUS-20071", true).is_empty());
        // A world spine is not the market's spine.
        assert!(urls(ArtKind::Lab, "SLPS-25002", true).is_empty());
        assert_eq!(
            urls(ArtKind::Ico, "SLPS-25002", false),
            ["https://x/disc(jp)"]
        );
        // Screenshots from anywhere, the game's market first.
        assert_eq!(
            urls(ArtKind::Scr, "SCES-50003", true),
            ["https://x/ss(eu)", "https://x/ss(wor)"]
        );
        assert_eq!(
            urls(ArtKind::Scr, "SLPS-25002", true),
            ["https://x/ss(wor)", "https://x/ss(eu)"]
        );
        // The HD logo first; a background without a region is fine.
        assert_eq!(
            urls(ArtKind::Lgo, "SLPS-25002", true),
            ["https://x/wheel-hd(jp)", "https://x/wheel(jp)"]
        );
        assert_eq!(
            urls(ArtKind::Bg, "SLUS-20071", false),
            ["https://x/fan%20art"]
        );
        // A serial with no market takes any packaging.
        assert_eq!(
            urls(ArtKind::Lab, "HOME_000.01", false),
            ["https://x/side(wor)"]
        );
    }

    #[test]
    fn credentials_never_show() {
        let api = Api {
            base: "https://api.example/api2".into(),
            dev: Some(DevCredentials {
                devid: "dev".into(),
                devpassword: "s3cret&x".into(),
                softname: "YAOPLM 0.1".into(),
            }),
        };
        let account = Account {
            user: "me".into(),
            password: "pw".into(),
        };
        let url = api.info_url(Some(&account), "SLPS-25002").unwrap();
        assert!(url.starts_with("https://api.example/api2/jeuInfos.php?devid=dev&devpassword=s3cret%26x&softname=YAOPLM%200.1&output=xml&ssid=me&sspassword=pw"), "{url}");
        assert!(
            url.ends_with("&systemeid=58&romtype=iso&romnom=SLPS-25002.iso&serialnum=SLPS-25002"),
            "{url}"
        );
        let shown = redact(&url);
        assert!(
            !shown.contains("s3cret") && !shown.contains("pw") && !shown.contains("=me"),
            "{shown}"
        );
        assert!(
            shown.contains("devid=***&devpassword=***") && shown.contains("serialnum=SLPS-25002")
        );
        assert_eq!(redact("https://x/y"), "https://x/y");
        assert_eq!(
            redact_text(
                "error sending request for url (https://a/b?ssid=me&sspassword=pw): timed out"
            ),
            "error sending request for url (https://a/b?ssid=***&sspassword=***): timed out"
        );
        assert_eq!(redact_text("a?b&c=d&devid="), "a?b&c=d&devid=***");
        assert!(!format!("{api:?}{account:?}").contains("s3cret"));
        assert!(Api { dev: None, ..api }.info_url(None, "X").is_none());
    }

    #[test]
    fn errors_pause_the_source() {
        let source = ScreenScraper::new(Api::default());
        assert_eq!(source.paused(), None);
        let (why, pause) = status_pause(430);
        assert!(why.contains("quota"));
        source.pause(&why, pause.unwrap());
        assert_eq!(source.paused().as_deref(), Some(why.as_str()));
        source.pause("short", Duration::ZERO);
        assert_eq!(source.paused(), None);
        assert_eq!(status_pause(400).1, None);
        assert!(status_pause(403).1.is_some());
    }
}
