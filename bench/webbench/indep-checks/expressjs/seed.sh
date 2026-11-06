#!/usr/bin/env bash
# Seed data the acceptance checks expect. Run after the app is up: ./seed.sh [port]
P=${1:-4300}
curl -s -X POST -H 'Content-Type: application/json' -d '{"name":"Blue Mug","price":1234,"image":"https://picsum.photos/200","description":"A blue mug","quantity":100}' http://localhost:$P/api/products; echo
curl -s -X POST -H 'Content-Type: application/json' -d '{"name":"Red Shirt","price":5678,"image":"https://picsum.photos/201","description":"A red shirt","quantity":50}' http://localhost:$P/api/products; echo
