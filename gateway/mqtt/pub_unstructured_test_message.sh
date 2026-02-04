mosquitto_pub -h 127.0.0.1 -p 1883 -t "fischertechnik/raw" -m \
'{ ( topic: förderband1-4, data: 1.2V, datetime: now), ( topic: förderband2, data: 1.1V, datetime: now) }'
