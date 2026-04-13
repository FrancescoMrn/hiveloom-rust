use clap::Args;

use super::client::ApiClient;

#[derive(Args)]
pub struct HealthArgs {
    /// API endpoint
    #[arg(long)]
    pub endpoint: Option<String>,

    /// Bearer token for remote API access
    #[arg(long)]
    pub token: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct DoctorArgs {
    /// Data directory to check
    #[arg(long, default_value = "/var/lib/hiveloom")]
    pub data_dir: String,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct StatusArgs {
    /// API endpoint
    #[arg(long)]
    pub endpoint: Option<String>,

    /// Bearer token for remote API access
    #[arg(long)]
    pub token: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

pub async fn run_health(args: HealthArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());

    let result = client.get_raw("/healthz").await;
    match result {
        Ok(status) if status.is_success() => {
            let out = serde_json::json!({ "status": "healthy", "code": status.as_u16() });
            if args.json {
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Hiveloom is healthy (HTTP {})", status.as_u16());
            }
        }
        Ok(status) => {
            let out = serde_json::json!({ "status": "unhealthy", "code": status.as_u16() });
            if args.json {
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                eprintln!("Hiveloom is unhealthy (HTTP {})", status.as_u16());
            }
            std::process::exit(1);
        }
        Err(e) => {
            let out = serde_json::json!({ "status": "unreachable", "error": e.to_string() });
            if args.json {
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                eprintln!("Cannot reach Hiveloom: {e}");
            }
            std::process::exit(1);
        }
    }

    Ok(())
}

pub async fn run_doctor(args: DoctorArgs) -> anyhow::Result<()> {
    let data_dir = std::path::Path::new(&args.data_dir);
    let mut checks: Vec<(&str, bool, String)> = Vec::new();

    // Check data directory exists and is writable
    let dir_ok = data_dir.is_dir();
    checks.push((
        "Data directory exists",
        dir_ok,
        if dir_ok {
            args.data_dir.clone()
        } else {
            format!("{} not found", args.data_dir)
        },
    ));

    if dir_ok {
        let writable = std::fs::metadata(data_dir)
            .map(|m| !m.permissions().readonly())
            .unwrap_or(false);
        checks.push((
            "Data directory writable",
            writable,
            if writable {
                "OK".to_string()
            } else {
                "Read-only".to_string()
            },
        ));
    }

    // Check master.key exists
    let key_path = data_dir.join("master.key");
    let key_ok = key_path.exists();
    checks.push((
        "master.key present",
        key_ok,
        if key_ok {
            "Found".to_string()
        } else {
            "Missing (will be auto-created on first run)".to_string()
        },
    ));

    // Check platform.db exists and is readable
    let db_path = data_dir.join("platform.db");
    let db_ok = db_path.exists();
    checks.push((
        "platform.db exists",
        db_ok,
        if db_ok {
            "Found".to_string()
        } else {
            "Missing (will be created on first run)".to_string()
        },
    ));

    // If platform.db exists, check SQLite integrity
    if db_ok {
        let integrity = check_sqlite_integrity(&db_path);
        checks.push((
            "platform.db integrity",
            integrity.0,
            integrity.1,
        ));
    }

    // Check tenants directory
    let tenants_dir = data_dir.join("tenants");
    let tenants_ok = tenants_dir.is_dir();
    let tenant_count = if tenants_ok {
        std::fs::read_dir(&tenants_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).count())
            .unwrap_or(0)
    } else {
        0
    };
    checks.push((
        "Tenants directory",
        tenants_ok || !db_ok, // OK if no DB yet
        if tenants_ok {
            format!("{} tenant(s)", tenant_count)
        } else {
            "Not found".to_string()
        },
    ));

    if args.json {
        let results: Vec<serde_json::Value> = checks
            .iter()
            .map(|(name, ok, detail)| {
                serde_json::json!({
                    "check": name,
                    "pass": ok,
                    "detail": detail,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        println!("Hiveloom Doctor\n");
        for (name, ok, detail) in &checks {
            let mark = if *ok { "PASS" } else { "FAIL" };
            println!("  [{mark}] {name}: {detail}");
        }
        let pass_count = checks.iter().filter(|c| c.1).count();
        println!("\n{}/{} checks passed", pass_count, checks.len());
    }

    Ok(())
}

fn check_sqlite_integrity(path: &std::path::Path) -> (bool, String) {
    match rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Ok(conn) => {
            match conn.query_row("PRAGMA integrity_check", [], |row| {
                row.get::<_, String>(0)
            }) {
                Ok(result) if result == "ok" => (true, "OK".to_string()),
                Ok(result) => (false, format!("Integrity issue: {result}")),
                Err(e) => (false, format!("Check failed: {e}")),
            }
        }
        Err(e) => (false, format!("Cannot open: {e}")),
    }
}

pub async fn run_status(args: StatusArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());

    // Probe healthz
    let health_ok = client.get_raw("/healthz").await.map(|s| s.is_success()).unwrap_or(false);

    // Try to get tenant count
    let tenants: Vec<serde_json::Value> = client
        .get("/api/tenants")
        .await
        .unwrap_or_default();

    if args.json {
        let out = serde_json::json!({
            "running": health_ok,
            "tenant_count": tenants.len(),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        let state = if health_ok { "running" } else { "not reachable" };
        println!("Service state:  {state}");
        println!("Tenants:        {}", tenants.len());
    }

    Ok(())
}
