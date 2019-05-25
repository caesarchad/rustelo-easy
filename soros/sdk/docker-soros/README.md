## Minimal Bitconch Docker image
This image is automatically updated by CI

https://hub.docker.com/r/bitconch/bus/

### Usage:
Run the latest beta image:
```bash
$ docker run --rm -p 8899:8899 bitconch/bus:beta
```

Run the latest nightly image:
```bash
$ docker run --rm -p 8899:8899 bitconch/bus:nightly
```

Port *8899* is the JSON RPC port, which is used by clients to communicate with the network.
