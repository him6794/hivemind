package nodepool;

import static io.grpc.MethodDescriptor.generateFullMethodName;
import static io.grpc.stub.ClientCalls.asyncBidiStreamingCall;
import static io.grpc.stub.ClientCalls.asyncClientStreamingCall;
import static io.grpc.stub.ClientCalls.asyncServerStreamingCall;
import static io.grpc.stub.ClientCalls.asyncUnaryCall;
import static io.grpc.stub.ClientCalls.blockingServerStreamingCall;
import static io.grpc.stub.ClientCalls.blockingUnaryCall;
import static io.grpc.stub.ClientCalls.futureUnaryCall;
import static io.grpc.stub.ServerCalls.asyncBidiStreamingCall;
import static io.grpc.stub.ServerCalls.asyncClientStreamingCall;
import static io.grpc.stub.ServerCalls.asyncServerStreamingCall;
import static io.grpc.stub.ServerCalls.asyncUnaryCall;
import static io.grpc.stub.ServerCalls.asyncUnimplementedStreamingCall;
import static io.grpc.stub.ServerCalls.asyncUnimplementedUnaryCall;

/**
 */
@javax.annotation.Generated(
    value = "by gRPC proto compiler (version 1.9.1)",
    comments = "Source: nodepool.proto")
public final class NodeManagerServiceGrpc {

  private NodeManagerServiceGrpc() {}

  public static final String SERVICE_NAME = "nodepool.NodeManagerService";

  // Static method descriptors that strictly reflect the proto.
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getRegisterWorkerNodeMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.RegisterWorkerNodeRequest,
      nodepool.StatusResponse> METHOD_REGISTER_WORKER_NODE = getRegisterWorkerNodeMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.RegisterWorkerNodeRequest,
      nodepool.StatusResponse> getRegisterWorkerNodeMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.RegisterWorkerNodeRequest,
      nodepool.StatusResponse> getRegisterWorkerNodeMethod() {
    io.grpc.MethodDescriptor<nodepool.RegisterWorkerNodeRequest, nodepool.StatusResponse> getRegisterWorkerNodeMethod;
    if ((getRegisterWorkerNodeMethod = NodeManagerServiceGrpc.getRegisterWorkerNodeMethod) == null) {
      synchronized (NodeManagerServiceGrpc.class) {
        if ((getRegisterWorkerNodeMethod = NodeManagerServiceGrpc.getRegisterWorkerNodeMethod) == null) {
          NodeManagerServiceGrpc.getRegisterWorkerNodeMethod = getRegisterWorkerNodeMethod = 
              io.grpc.MethodDescriptor.<nodepool.RegisterWorkerNodeRequest, nodepool.StatusResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.NodeManagerService", "RegisterWorkerNode"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.RegisterWorkerNodeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StatusResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new NodeManagerServiceMethodDescriptorSupplier("RegisterWorkerNode"))
                  .build();
          }
        }
     }
     return getRegisterWorkerNodeMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getHealthCheckMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.HealthCheckRequest,
      nodepool.HealthCheckResponse> METHOD_HEALTH_CHECK = getHealthCheckMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.HealthCheckRequest,
      nodepool.HealthCheckResponse> getHealthCheckMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.HealthCheckRequest,
      nodepool.HealthCheckResponse> getHealthCheckMethod() {
    io.grpc.MethodDescriptor<nodepool.HealthCheckRequest, nodepool.HealthCheckResponse> getHealthCheckMethod;
    if ((getHealthCheckMethod = NodeManagerServiceGrpc.getHealthCheckMethod) == null) {
      synchronized (NodeManagerServiceGrpc.class) {
        if ((getHealthCheckMethod = NodeManagerServiceGrpc.getHealthCheckMethod) == null) {
          NodeManagerServiceGrpc.getHealthCheckMethod = getHealthCheckMethod = 
              io.grpc.MethodDescriptor.<nodepool.HealthCheckRequest, nodepool.HealthCheckResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.NodeManagerService", "HealthCheck"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.HealthCheckRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.HealthCheckResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new NodeManagerServiceMethodDescriptorSupplier("HealthCheck"))
                  .build();
          }
        }
     }
     return getHealthCheckMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getReportStatusMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.ReportStatusRequest,
      nodepool.StatusResponse> METHOD_REPORT_STATUS = getReportStatusMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.ReportStatusRequest,
      nodepool.StatusResponse> getReportStatusMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.ReportStatusRequest,
      nodepool.StatusResponse> getReportStatusMethod() {
    io.grpc.MethodDescriptor<nodepool.ReportStatusRequest, nodepool.StatusResponse> getReportStatusMethod;
    if ((getReportStatusMethod = NodeManagerServiceGrpc.getReportStatusMethod) == null) {
      synchronized (NodeManagerServiceGrpc.class) {
        if ((getReportStatusMethod = NodeManagerServiceGrpc.getReportStatusMethod) == null) {
          NodeManagerServiceGrpc.getReportStatusMethod = getReportStatusMethod = 
              io.grpc.MethodDescriptor.<nodepool.ReportStatusRequest, nodepool.StatusResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.NodeManagerService", "ReportStatus"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.ReportStatusRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StatusResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new NodeManagerServiceMethodDescriptorSupplier("ReportStatus"))
                  .build();
          }
        }
     }
     return getReportStatusMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getGetNodeListMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.GetNodeListRequest,
      nodepool.GetNodeListResponse> METHOD_GET_NODE_LIST = getGetNodeListMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.GetNodeListRequest,
      nodepool.GetNodeListResponse> getGetNodeListMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.GetNodeListRequest,
      nodepool.GetNodeListResponse> getGetNodeListMethod() {
    io.grpc.MethodDescriptor<nodepool.GetNodeListRequest, nodepool.GetNodeListResponse> getGetNodeListMethod;
    if ((getGetNodeListMethod = NodeManagerServiceGrpc.getGetNodeListMethod) == null) {
      synchronized (NodeManagerServiceGrpc.class) {
        if ((getGetNodeListMethod = NodeManagerServiceGrpc.getGetNodeListMethod) == null) {
          NodeManagerServiceGrpc.getGetNodeListMethod = getGetNodeListMethod = 
              io.grpc.MethodDescriptor.<nodepool.GetNodeListRequest, nodepool.GetNodeListResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.NodeManagerService", "GetNodeList"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.GetNodeListRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.GetNodeListResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new NodeManagerServiceMethodDescriptorSupplier("GetNodeList"))
                  .build();
          }
        }
     }
     return getGetNodeListMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static NodeManagerServiceStub newStub(io.grpc.Channel channel) {
    return new NodeManagerServiceStub(channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static NodeManagerServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    return new NodeManagerServiceBlockingStub(channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static NodeManagerServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    return new NodeManagerServiceFutureStub(channel);
  }

  /**
   */
  public static abstract class NodeManagerServiceImplBase implements io.grpc.BindableService {

    /**
     */
    public void registerWorkerNode(nodepool.RegisterWorkerNodeRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getRegisterWorkerNodeMethod(), responseObserver);
    }

    /**
     */
    public void healthCheck(nodepool.HealthCheckRequest request,
        io.grpc.stub.StreamObserver<nodepool.HealthCheckResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getHealthCheckMethod(), responseObserver);
    }

    /**
     */
    public void reportStatus(nodepool.ReportStatusRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getReportStatusMethod(), responseObserver);
    }

    /**
     */
    public void getNodeList(nodepool.GetNodeListRequest request,
        io.grpc.stub.StreamObserver<nodepool.GetNodeListResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getGetNodeListMethod(), responseObserver);
    }

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return io.grpc.ServerServiceDefinition.builder(getServiceDescriptor())
          .addMethod(
            getRegisterWorkerNodeMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.RegisterWorkerNodeRequest,
                nodepool.StatusResponse>(
                  this, METHODID_REGISTER_WORKER_NODE)))
          .addMethod(
            getHealthCheckMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.HealthCheckRequest,
                nodepool.HealthCheckResponse>(
                  this, METHODID_HEALTH_CHECK)))
          .addMethod(
            getReportStatusMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.ReportStatusRequest,
                nodepool.StatusResponse>(
                  this, METHODID_REPORT_STATUS)))
          .addMethod(
            getGetNodeListMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.GetNodeListRequest,
                nodepool.GetNodeListResponse>(
                  this, METHODID_GET_NODE_LIST)))
          .build();
    }
  }

  /**
   */
  public static final class NodeManagerServiceStub extends io.grpc.stub.AbstractStub<NodeManagerServiceStub> {
    private NodeManagerServiceStub(io.grpc.Channel channel) {
      super(channel);
    }

    private NodeManagerServiceStub(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected NodeManagerServiceStub build(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      return new NodeManagerServiceStub(channel, callOptions);
    }

    /**
     */
    public void registerWorkerNode(nodepool.RegisterWorkerNodeRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getRegisterWorkerNodeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void healthCheck(nodepool.HealthCheckRequest request,
        io.grpc.stub.StreamObserver<nodepool.HealthCheckResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getHealthCheckMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void reportStatus(nodepool.ReportStatusRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getReportStatusMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getNodeList(nodepool.GetNodeListRequest request,
        io.grpc.stub.StreamObserver<nodepool.GetNodeListResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getGetNodeListMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   */
  public static final class NodeManagerServiceBlockingStub extends io.grpc.stub.AbstractStub<NodeManagerServiceBlockingStub> {
    private NodeManagerServiceBlockingStub(io.grpc.Channel channel) {
      super(channel);
    }

    private NodeManagerServiceBlockingStub(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected NodeManagerServiceBlockingStub build(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      return new NodeManagerServiceBlockingStub(channel, callOptions);
    }

    /**
     */
    public nodepool.StatusResponse registerWorkerNode(nodepool.RegisterWorkerNodeRequest request) {
      return blockingUnaryCall(
          getChannel(), getRegisterWorkerNodeMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.HealthCheckResponse healthCheck(nodepool.HealthCheckRequest request) {
      return blockingUnaryCall(
          getChannel(), getHealthCheckMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.StatusResponse reportStatus(nodepool.ReportStatusRequest request) {
      return blockingUnaryCall(
          getChannel(), getReportStatusMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.GetNodeListResponse getNodeList(nodepool.GetNodeListRequest request) {
      return blockingUnaryCall(
          getChannel(), getGetNodeListMethod(), getCallOptions(), request);
    }
  }

  /**
   */
  public static final class NodeManagerServiceFutureStub extends io.grpc.stub.AbstractStub<NodeManagerServiceFutureStub> {
    private NodeManagerServiceFutureStub(io.grpc.Channel channel) {
      super(channel);
    }

    private NodeManagerServiceFutureStub(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected NodeManagerServiceFutureStub build(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      return new NodeManagerServiceFutureStub(channel, callOptions);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.StatusResponse> registerWorkerNode(
        nodepool.RegisterWorkerNodeRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getRegisterWorkerNodeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.HealthCheckResponse> healthCheck(
        nodepool.HealthCheckRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getHealthCheckMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.StatusResponse> reportStatus(
        nodepool.ReportStatusRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getReportStatusMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.GetNodeListResponse> getNodeList(
        nodepool.GetNodeListRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getGetNodeListMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_REGISTER_WORKER_NODE = 0;
  private static final int METHODID_HEALTH_CHECK = 1;
  private static final int METHODID_REPORT_STATUS = 2;
  private static final int METHODID_GET_NODE_LIST = 3;

  private static final class MethodHandlers<Req, Resp> implements
      io.grpc.stub.ServerCalls.UnaryMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ServerStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ClientStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.BidiStreamingMethod<Req, Resp> {
    private final NodeManagerServiceImplBase serviceImpl;
    private final int methodId;

    MethodHandlers(NodeManagerServiceImplBase serviceImpl, int methodId) {
      this.serviceImpl = serviceImpl;
      this.methodId = methodId;
    }

    @java.lang.Override
    @java.lang.SuppressWarnings("unchecked")
    public void invoke(Req request, io.grpc.stub.StreamObserver<Resp> responseObserver) {
      switch (methodId) {
        case METHODID_REGISTER_WORKER_NODE:
          serviceImpl.registerWorkerNode((nodepool.RegisterWorkerNodeRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.StatusResponse>) responseObserver);
          break;
        case METHODID_HEALTH_CHECK:
          serviceImpl.healthCheck((nodepool.HealthCheckRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.HealthCheckResponse>) responseObserver);
          break;
        case METHODID_REPORT_STATUS:
          serviceImpl.reportStatus((nodepool.ReportStatusRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.StatusResponse>) responseObserver);
          break;
        case METHODID_GET_NODE_LIST:
          serviceImpl.getNodeList((nodepool.GetNodeListRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.GetNodeListResponse>) responseObserver);
          break;
        default:
          throw new AssertionError();
      }
    }

    @java.lang.Override
    @java.lang.SuppressWarnings("unchecked")
    public io.grpc.stub.StreamObserver<Req> invoke(
        io.grpc.stub.StreamObserver<Resp> responseObserver) {
      switch (methodId) {
        default:
          throw new AssertionError();
      }
    }
  }

  private static abstract class NodeManagerServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    NodeManagerServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return nodepool.NodepoolOuterClass.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("NodeManagerService");
    }
  }

  private static final class NodeManagerServiceFileDescriptorSupplier
      extends NodeManagerServiceBaseDescriptorSupplier {
    NodeManagerServiceFileDescriptorSupplier() {}
  }

  private static final class NodeManagerServiceMethodDescriptorSupplier
      extends NodeManagerServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final String methodName;

    NodeManagerServiceMethodDescriptorSupplier(String methodName) {
      this.methodName = methodName;
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.MethodDescriptor getMethodDescriptor() {
      return getServiceDescriptor().findMethodByName(methodName);
    }
  }

  private static volatile io.grpc.ServiceDescriptor serviceDescriptor;

  public static io.grpc.ServiceDescriptor getServiceDescriptor() {
    io.grpc.ServiceDescriptor result = serviceDescriptor;
    if (result == null) {
      synchronized (NodeManagerServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new NodeManagerServiceFileDescriptorSupplier())
              .addMethod(getRegisterWorkerNodeMethod())
              .addMethod(getHealthCheckMethod())
              .addMethod(getReportStatusMethod())
              .addMethod(getGetNodeListMethod())
              .build();
        }
      }
    }
    return result;
  }
}
