mosquitto_pub -h 127.0.0.1 -p 1883 -t "fischertechnik/events" -m \
'{"schema":"ft.events.v1","ts":"2026-01-12T20:41:12Z","source":{"machine_id":"ft-01","line":"line-a"},"event":{"kind":"machine_failure","data":{"component":"cog","failure_code":"stuck","severity":"error","message":"Cog jam detected"}}}'
