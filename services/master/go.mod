module hivemind/services/master

go 1.25.0

require (
	google.golang.org/grpc v1.66.0
	hivemind/services/nodepool v0.0.0
)

require (
	golang.org/x/net v0.51.0 // indirect
	golang.org/x/sys v0.42.0 // indirect
	golang.org/x/text v0.35.0 // indirect
	google.golang.org/genproto/googleapis/rpc v0.0.0-20240604185151-ef581f913117 // indirect
	google.golang.org/protobuf v1.34.2 // indirect
)

replace hivemind/services/nodepool => ../nodepool
