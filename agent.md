You are implementing code for the HiveMind distributed computing platform.

Repository structure:

hivemind/

docs/
proto/

executor-rs/

frontend/

services/
master/
nodepool/
worker/

Each service is an independent Go module.

Inside each service the structure is:

cmd/server/main.go

internal/
handler/
service/
repository/
models/

Architecture rules:

handler → service → repository

handler:

* gRPC layer
* converts protobuf messages to internal models
* calls service
* must NOT access repository

service:

* business logic
* calls repository

repository:

* storage layer only
* no business logic

Strict Go rules:

* NEVER use interface{} in handlers
* NEVER return map[string]string
* All API responses must be protobuf messages
* Use context.Context in service methods
* Use structured errors
* Do not invent imports
* Only import packages that exist in the repository

When implementing code:

* Only generate the requested file
* The code must compile
* Follow idiomatic Go conventions
* Do not generate example code

Before finishing verify:

* no interface{}
* no handler calling repository
* valid imports
