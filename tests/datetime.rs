//! stdlib/time.ax + datetime.ax + calendar.ax tests (v0.44.0).
//! Hinnant civil-date round trips (epoch, Y2K, pre-1970 via floor_div),
//! weekday table, leap years, the strftime subset, GetLocalTime sanity,
//! QPC monotonic timing with a 50 ms sleep, and the month-grid renderer.
use std::process::Command;

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

fn abs(rel: &str) -> String {
    // forward slashes: backslashes would read as escape sequences inside
    // the Aoxn import string literal
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(rel)
        .display()
        .to_string()
        .replace('\\', "/")
}

const SRC: &str = r#"
import * from "stdlib/datetime.ax"
import * from "stdlib/calendar.ax"
import * from "stdlib/time.ax"

def main() -> int:
    fails = 0
    # epoch
    dt = datetime_from_unix(0)
    if dt.year == 1970 and dt.month == 1 and dt.day == 1 and dt.hour == 0 and dt.minute == 0 and dt.second == 0:
        print("PASS epoch-ymdhms")
    else:
        print("FAIL epoch-ymdhms")
        fails = fails + 1
    if datetime_weekday(dt) == 3:
        print("PASS epoch-thursday")
    else:
        print("FAIL epoch-thursday " + str(datetime_weekday(dt)))
        fails = fails + 1
    # 2000-03-01 00:00:00 UTC
    y2k = datetime_from_unix(951868800)
    if y2k.year == 2000 and y2k.month == 3 and y2k.day == 1:
        print("PASS y2k-date")
    else:
        print("FAIL y2k-date " + str(y2k.year) + "-" + str(y2k.month) + "-" + str(y2k.day))
        fails = fails + 1
    if datetime_to_unix(y2k) == 951868800:
        print("PASS y2k-roundtrip")
    else:
        print("FAIL y2k-roundtrip " + str(datetime_to_unix(y2k)))
        fails = fails + 1
    if datetime_to_unix(datetime_from_unix(1234567890)) == 1234567890:
        print("PASS roundtrip-1234567890")
    else:
        print("FAIL roundtrip-1234567890")
        fails = fails + 1
    # pre-1970 dates exercise floor_div
    dt69 = datetime_from_unix(-1)
    if dt69.year == 1969 and dt69.month == 12 and dt69.day == 31 and dt69.hour == 23 and dt69.minute == 59 and dt69.second == 59:
        print("PASS pre-epoch")
    else:
        print("FAIL pre-epoch")
        fails = fails + 1
    # 2026-10-05 is a Monday
    mon = DateTime(year=2026, month=10, day=5, hour=12, minute=34, second=56)
    if datetime_weekday(mon) == 0:
        print("PASS 2026-10-05-monday")
    else:
        print("FAIL 2026-10-05-monday " + str(datetime_weekday(mon)))
        fails = fails + 1
    # leap years / month lengths
    if datetime_is_leap(2000) and datetime_is_leap(2024) and not datetime_is_leap(1900) and not datetime_is_leap(2023):
        print("PASS leap")
    else:
        print("FAIL leap")
        fails = fails + 1
    if datetime_days_in_month(2024, 2) == 29 and datetime_days_in_month(2023, 2) == 28 and datetime_days_in_month(2026, 4) == 30 and datetime_days_in_month(2026, 12) == 31:
        print("PASS days-in-month")
    else:
        print("FAIL days-in-month")
        fails = fails + 1
    # formatting
    if datetime_format(y2k, "%Y-%m-%d %H:%M:%S") == "2000-03-01 00:00:00":
        print("PASS fmt-iso")
    else:
        print("FAIL fmt-iso " + datetime_format(y2k, "%Y-%m-%d %H:%M:%S"))
        fails = fails + 1
    if datetime_format(y2k, "%j|%A|%B|%a|%b|%y|%%") == "061|Wednesday|March|Wed|Mar|00|%":
        print("PASS fmt-specs")
    else:
        print("FAIL fmt-specs " + datetime_format(y2k, "%j|%A|%B|%a|%b|%y|%%"))
        fails = fails + 1
    if datetime_format(mon, "%Y%m%d%H%M%S") == "20261005123456":
        print("PASS fmt-compact")
    else:
        print("FAIL fmt-compact " + datetime_format(mon, "%Y%m%d%H%M%S"))
        fails = fails + 1
    # now: local and UTC agree within a day of wall-clock sanity
    now = datetime_now()
    if now.year >= 2026 and now.year <= 2035:
        print("PASS now-year")
    else:
        print("FAIL now-year " + str(now.year))
        fails = fails + 1
    if len(datetime_iso(now)) == 19:
        print("PASS now-iso-len")
    else:
        print("FAIL now-iso-len " + datetime_iso(now))
        fails = fails + 1
    u = datetime_utcnow()
    if u.year >= 2026 and u.year <= 2035:
        print("PASS utcnow-year")
    else:
        print("FAIL utcnow-year " + str(u.year))
        fails = fails + 1
    # time module
    t = time_unix()
    if t > 1750000000 and t < 2000000000:
        print("PASS time-unix-sane")
    else:
        print("FAIL time-unix-sane " + str(t))
        fails = fails + 1
    ms = time_unix_ms()
    if ms > t * 1000 - 2000 and ms < t * 1000 + 60000:
        print("PASS time-ms-agrees")
    else:
        print("FAIL time-ms-agrees " + str(t) + " vs " + str(ms))
        fails = fails + 1
    n1 = time_now_ns()
    time_sleep(0.05)
    n2 = time_now_ns()
    if n2 - n1 >= 40000000:
        print("PASS qpc-sleep-50ms")
    else:
        print("FAIL qpc-sleep-50ms " + str(n2 - n1))
        fails = fails + 1
    # calendar
    r = cal_monthrange(2026, 10)
    if r.wday == 3 and r.days == 31:
        print("PASS monthrange-2026-10")
    else:
        print("FAIL monthrange-2026-10 " + str(r.wday) + "," + str(r.days))
        fails = fails + 1
    grid = cal_month(2026, 10)
    if find(grid, "October 2026") and find(grid, "Mo Tu We Th Fr Sa Su") and find(grid, " 1  2  3  4"):
        print("PASS cal-month-grid")
    else:
        print("FAIL cal-month-grid")
        print(grid)
        fails = fails + 1
    w = cal_monthcalendar(2026, 10)
    row0 = vecvec_get(w, 0)
    if vec_get(row0, 0) == 0 and vec_get(row0, 3) == 1 and vec_get(row0, 6) == 4:
        print("PASS cal-monthcalendar")
    else:
        print("FAIL cal-monthcalendar")
        fails = fails + 1
    print("DONE fails=" + str(fails))
    return fails

# substring containment (str_sub leak is fine in a test driver)
def find(s: string, needle: string) -> bool:
    i = 0
    while i + len(needle) <= len(s):
        if str_sub(s, i, i + len(needle)) == needle:
            return True
        i = i + 1
    return False

"#;

#[test]
fn datetime_module_conversions() {
    if aoxn::find_clang().is_none() {
        eprintln!("skipping: clang not found (set AOXN_CLANG or add clang to PATH)");
        return;
    }
    // the drivers import stdlib modules BY NAME, which exercises the
    // AOXN_STDLIB resolution path (env -> <root>/lib/stdlib -> checkout)
    std::env::set_var("AOXN_STDLIB", abs("stdlib"));
    let dir = std::env::temp_dir().join(format!("aoxn-datetime_module_conversions-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src_path = dir.join("datetime_module_conversions.ax");
    let exe = dir.join(format!("datetime_module_conversions{EXE}"));
    std::fs::write(&src_path, SRC).unwrap();
    aoxn::build_paths_opts(&[src_path.display().to_string()], &exe, true, &[], &[])
        .unwrap_or_else(|d| panic!("driver failed to compile: {d:?}"));
    let out =
    Command::new(&exe).output().expect("failed to run driver");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "driver exited {:?}
stdout:
{text}
stderr:
{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("DONE fails=0"), "driver reported failures:
{text}");
    assert!(!text.contains("FAIL"), "driver printed FAIL:
{text}");
    for marker in [
        "epoch-ymdhms",
        "epoch-thursday",
        "y2k-date",
        "y2k-roundtrip",
        "roundtrip-1234567890",
        "pre-epoch",
        "2026-10-05-monday",
        "leap",
        "days-in-month",
        "fmt-iso",
        "fmt-specs",
        "fmt-compact",
        "now-year",
        "now-iso-len",
        "utcnow-year",
        "time-unix-sane",
        "time-ms-agrees",
        "qpc-sleep-50ms",
        "monthrange-2026-10",
        "cal-month-grid",
        "cal-monthcalendar",
    ] {
        assert!(
            text.contains(&format!("PASS {marker}")),
            "missing PASS {marker}
{text}"
        );
    }
}
