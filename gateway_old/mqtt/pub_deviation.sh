mosquitto_pub -h 127.0.0.1 -p 1883 -t "fischertechnik/events" -m \
'{"schema":"ft.events.v1","ts":"2026-01-12T20:41:12Z","source":{"machine_id":"ft-01"},"event":{"kind":"counter_deviation","data":{"counter_name":"parts_counter","expected":10,"actual":7,"tolerance":1,"severity":"warning"}}}'
