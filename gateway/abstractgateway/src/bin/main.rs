use std::collections::HashMap;

use abstractgateway; // rename crate if needed
use abstractgateway::downstream::configuration::Configuration;
use abstractgateway::IDownstream;
use abstractgateway::downstream::configuration::Id;
use abstractgateway::downstream::Property;
use abstractgateway::upstream::Property as USProperty;
use abstractgateway::downstream::Method;

use serde_json::Value;

fn usage() -> ! {
    eprintln!(
        r#"Commands:
  set-config <ip> <username> <password> [settings_json]
  set-prop   <id> <type> <value_json>
  set-method <id> <variables_json>
  exec       <method_id>
  show

Examples:
  cargo run --bin main -- set-config 127.0.0.1 user pass '{{"qos":1}}'
  cargo run --bin main -- set-prop motor/speed float 1200
  cargo run --bin main -- set-prop sensor/temp float 36.5
  cargo run --bin main -- set-method calibrate '{{"target":42,"mode":"fast"}}'
  cargo run --bin main -- exec calibrate
  cargo run --bin main -- show
"#
    );
    std::process::exit(2);
}

fn parse_json(arg: &str) -> Value {
    // Allow simple JSON values like 1200, "abc", true, {"a":1}
    serde_json::from_str(arg).unwrap_or_else(|e| {
        eprintln!("Invalid JSON: {arg}\nError: {e}");
        std::process::exit(2);
    })
}

fn main() {
    // In a real test harness you might persist state; for now it's one-run.
    // (If you want a REPL-like session, say so and I’ll adapt it.)
    let mut ds = IDownstream::new();

    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        usage();
    }

    let cmd = args.remove(0);

    match cmd.as_str() {
        "set-config" => {
            if args.len() < 3 {
                usage();
            }
            let ipaddress = args.remove(0);
            let username = args.remove(0);
            let password = args.remove(0);

            let settings = if !args.is_empty() {
                let v = parse_json(&args.remove(0));
                // Expect object -> map; if not object, store under "default"
                match v {
                    Value::Object(map) => map
                        .into_iter()
                        .map(|(k, v)| (k, v))
                        .collect::<HashMap<String, Value>>(),
                    other => {
                        let mut m = HashMap::new();
                        m.insert("default".to_string(), other);
                        m
                    }
                }
            } else {
                HashMap::new()
            };

            let conf = Configuration {
                ipaddress,
                username,
                password,
                settings,
            };
            ds.set_configuration(conf);
            println!("OK: configuration set");
        }

        "set-prop" => {
            if args.len() < 3 {
                usage();
            }
            let id: Id = args.remove(0);
            let ty = args.remove(0);
            let value = parse_json(&args.remove(0));

            ds.upsert_property(id.clone(), Property { ty, value });
            println!("OK: property upserted: {id}");
        }

        "set-method" => {
            if args.len() < 2 {
                usage();
            }
            let id: Id = args.remove(0);
            let variables_json = parse_json(&args.remove(0));

            let variables = match variables_json {
                Value::Object(map) => map
                    .into_iter()
                    .map(|(k, v)| (k, v))
                    .collect::<HashMap<String, Value>>(),
                _ => {
                    eprintln!("variables_json must be a JSON object, e.g. '{{\"x\":1}}'");
                    std::process::exit(2);
                }
            };

            ds.upsert_method(id.clone(), Method { variables });
            println!("OK: method upserted: {id}");
        }

        "exec" => {
            if args.len() < 1 {
                usage();
            }
            let id: Id = args.remove(0);

            let m = ds.method(&id).cloned();
            match m {
                None => {
                    eprintln!("ERR: unknown method id: {id}");
                    std::process::exit(1);
                }
                Some(method) => {
                    // This is the "test execute" behavior:
                    // For now, we just print what would be executed.
                    println!("EXECUTE: {id}");
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&method.variables).unwrap()
                    );
                }
            }
        }

        "show" => {
            println!("=== Downstream ===");
            if let Some(c) = ds.configuration() {
                println!("Configuration:");
                println!(
                    "{}",
                    serde_json::to_string_pretty(c).unwrap_or_else(|_| format!("{c:?}"))
                );
            } else {
                println!("Configuration: <none>");
            }

            println!("\nProperties:");
            if ds.properties.is_empty() {
                println!("  <none>");
            } else {
                for (id, p) in &ds.properties {
                    println!("  {id}: type={} value={}", p.ty, p.value);
                }
            }

            println!("\nMethods:");
            if ds.methods.is_empty() {
                println!("  <none>");
            } else {
                for (id, m) in &ds.methods {
                    println!("  {id}: variables={}", serde_json::to_string(&m.variables).unwrap());
                }
            }
        }

        _ => usage(),
    }
}
