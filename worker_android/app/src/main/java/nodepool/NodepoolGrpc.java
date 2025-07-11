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
public final class NodepoolGrpc {

  private NodepoolGrpc() {}

  public static final String SERVICE_NAME = "nodepool.Nodepool";

  // Static method descriptors that strictly reflect the proto.
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getLoginMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.LoginRequest,
      nodepool.LoginResponse> METHOD_LOGIN = getLoginMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.LoginRequest,
      nodepool.LoginResponse> getLoginMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.LoginRequest,
      nodepool.LoginResponse> getLoginMethod() {
    io.grpc.MethodDescriptor<nodepool.LoginRequest, nodepool.LoginResponse> getLoginMethod;
    if ((getLoginMethod = NodepoolGrpc.getLoginMethod) == null) {
      synchronized (NodepoolGrpc.class) {
        if ((getLoginMethod = NodepoolGrpc.getLoginMethod) == null) {
          NodepoolGrpc.getLoginMethod = getLoginMethod = 
              io.grpc.MethodDescriptor.<nodepool.LoginRequest, nodepool.LoginResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.Nodepool", "Login"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.LoginRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.LoginResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new NodepoolMethodDescriptorSupplier("Login"))
                  .build();
          }
        }
     }
     return getLoginMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getRegisterWorkerNodeMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.RegisterWorkerNodeRequest,
      nodepool.RegisterWorkerNodeResponse> METHOD_REGISTER_WORKER_NODE = getRegisterWorkerNodeMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.RegisterWorkerNodeRequest,
      nodepool.RegisterWorkerNodeResponse> getRegisterWorkerNodeMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.RegisterWorkerNodeRequest,
      nodepool.RegisterWorkerNodeResponse> getRegisterWorkerNodeMethod() {
    io.grpc.MethodDescriptor<nodepool.RegisterWorkerNodeRequest, nodepool.RegisterWorkerNodeResponse> getRegisterWorkerNodeMethod;
    if ((getRegisterWorkerNodeMethod = NodepoolGrpc.getRegisterWorkerNodeMethod) == null) {
      synchronized (NodepoolGrpc.class) {
        if ((getRegisterWorkerNodeMethod = NodepoolGrpc.getRegisterWorkerNodeMethod) == null) {
          NodepoolGrpc.getRegisterWorkerNodeMethod = getRegisterWorkerNodeMethod = 
              io.grpc.MethodDescriptor.<nodepool.RegisterWorkerNodeRequest, nodepool.RegisterWorkerNodeResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.Nodepool", "RegisterWorkerNode"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.RegisterWorkerNodeRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.RegisterWorkerNodeResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new NodepoolMethodDescriptorSupplier("RegisterWorkerNode"))
                  .build();
          }
        }
     }
     return getRegisterWorkerNodeMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getReportStatusMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.ReportStatusRequest,
      nodepool.ReportStatusResponse> METHOD_REPORT_STATUS = getReportStatusMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.ReportStatusRequest,
      nodepool.ReportStatusResponse> getReportStatusMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.ReportStatusRequest,
      nodepool.ReportStatusResponse> getReportStatusMethod() {
    io.grpc.MethodDescriptor<nodepool.ReportStatusRequest, nodepool.ReportStatusResponse> getReportStatusMethod;
    if ((getReportStatusMethod = NodepoolGrpc.getReportStatusMethod) == null) {
      synchronized (NodepoolGrpc.class) {
        if ((getReportStatusMethod = NodepoolGrpc.getReportStatusMethod) == null) {
          NodepoolGrpc.getReportStatusMethod = getReportStatusMethod = 
              io.grpc.MethodDescriptor.<nodepool.ReportStatusRequest, nodepool.ReportStatusResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.Nodepool", "ReportStatus"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.ReportStatusRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.ReportStatusResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new NodepoolMethodDescriptorSupplier("ReportStatus"))
                  .build();
          }
        }
     }
     return getReportStatusMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getReturnTaskResultMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.ReturnTaskResultRequest,
      nodepool.ReturnTaskResultResponse> METHOD_RETURN_TASK_RESULT = getReturnTaskResultMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.ReturnTaskResultRequest,
      nodepool.ReturnTaskResultResponse> getReturnTaskResultMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.ReturnTaskResultRequest,
      nodepool.ReturnTaskResultResponse> getReturnTaskResultMethod() {
    io.grpc.MethodDescriptor<nodepool.ReturnTaskResultRequest, nodepool.ReturnTaskResultResponse> getReturnTaskResultMethod;
    if ((getReturnTaskResultMethod = NodepoolGrpc.getReturnTaskResultMethod) == null) {
      synchronized (NodepoolGrpc.class) {
        if ((getReturnTaskResultMethod = NodepoolGrpc.getReturnTaskResultMethod) == null) {
          NodepoolGrpc.getReturnTaskResultMethod = getReturnTaskResultMethod = 
              io.grpc.MethodDescriptor.<nodepool.ReturnTaskResultRequest, nodepool.ReturnTaskResultResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.Nodepool", "ReturnTaskResult"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.ReturnTaskResultRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.ReturnTaskResultResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new NodepoolMethodDescriptorSupplier("ReturnTaskResult"))
                  .build();
          }
        }
     }
     return getReturnTaskResultMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getTaskCompletedMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.TaskCompletedRequest,
      nodepool.TaskCompletedResponse> METHOD_TASK_COMPLETED = getTaskCompletedMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.TaskCompletedRequest,
      nodepool.TaskCompletedResponse> getTaskCompletedMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.TaskCompletedRequest,
      nodepool.TaskCompletedResponse> getTaskCompletedMethod() {
    io.grpc.MethodDescriptor<nodepool.TaskCompletedRequest, nodepool.TaskCompletedResponse> getTaskCompletedMethod;
    if ((getTaskCompletedMethod = NodepoolGrpc.getTaskCompletedMethod) == null) {
      synchronized (NodepoolGrpc.class) {
        if ((getTaskCompletedMethod = NodepoolGrpc.getTaskCompletedMethod) == null) {
          NodepoolGrpc.getTaskCompletedMethod = getTaskCompletedMethod = 
              io.grpc.MethodDescriptor.<nodepool.TaskCompletedRequest, nodepool.TaskCompletedResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.Nodepool", "TaskCompleted"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.TaskCompletedRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.TaskCompletedResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new NodepoolMethodDescriptorSupplier("TaskCompleted"))
                  .build();
          }
        }
     }
     return getTaskCompletedMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static NodepoolStub newStub(io.grpc.Channel channel) {
    return new NodepoolStub(channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static NodepoolBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    return new NodepoolBlockingStub(channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static NodepoolFutureStub newFutureStub(
      io.grpc.Channel channel) {
    return new NodepoolFutureStub(channel);
  }

  /**
   */
  public static abstract class NodepoolImplBase implements io.grpc.BindableService {

    /**
     */
    public void login(nodepool.LoginRequest request,
        io.grpc.stub.StreamObserver<nodepool.LoginResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getLoginMethod(), responseObserver);
    }

    /**
     */
    public void registerWorkerNode(nodepool.RegisterWorkerNodeRequest request,
        io.grpc.stub.StreamObserver<nodepool.RegisterWorkerNodeResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getRegisterWorkerNodeMethod(), responseObserver);
    }

    /**
     */
    public void reportStatus(nodepool.ReportStatusRequest request,
        io.grpc.stub.StreamObserver<nodepool.ReportStatusResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getReportStatusMethod(), responseObserver);
    }

    /**
     */
    public void returnTaskResult(nodepool.ReturnTaskResultRequest request,
        io.grpc.stub.StreamObserver<nodepool.ReturnTaskResultResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getReturnTaskResultMethod(), responseObserver);
    }

    /**
     */
    public void taskCompleted(nodepool.TaskCompletedRequest request,
        io.grpc.stub.StreamObserver<nodepool.TaskCompletedResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getTaskCompletedMethod(), responseObserver);
    }

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return io.grpc.ServerServiceDefinition.builder(getServiceDescriptor())
          .addMethod(
            getLoginMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.LoginRequest,
                nodepool.LoginResponse>(
                  this, METHODID_LOGIN)))
          .addMethod(
            getRegisterWorkerNodeMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.RegisterWorkerNodeRequest,
                nodepool.RegisterWorkerNodeResponse>(
                  this, METHODID_REGISTER_WORKER_NODE)))
          .addMethod(
            getReportStatusMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.ReportStatusRequest,
                nodepool.ReportStatusResponse>(
                  this, METHODID_REPORT_STATUS)))
          .addMethod(
            getReturnTaskResultMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.ReturnTaskResultRequest,
                nodepool.ReturnTaskResultResponse>(
                  this, METHODID_RETURN_TASK_RESULT)))
          .addMethod(
            getTaskCompletedMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.TaskCompletedRequest,
                nodepool.TaskCompletedResponse>(
                  this, METHODID_TASK_COMPLETED)))
          .build();
    }
  }

  /**
   */
  public static final class NodepoolStub extends io.grpc.stub.AbstractStub<NodepoolStub> {
    private NodepoolStub(io.grpc.Channel channel) {
      super(channel);
    }

    private NodepoolStub(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected NodepoolStub build(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      return new NodepoolStub(channel, callOptions);
    }

    /**
     */
    public void login(nodepool.LoginRequest request,
        io.grpc.stub.StreamObserver<nodepool.LoginResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getLoginMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void registerWorkerNode(nodepool.RegisterWorkerNodeRequest request,
        io.grpc.stub.StreamObserver<nodepool.RegisterWorkerNodeResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getRegisterWorkerNodeMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void reportStatus(nodepool.ReportStatusRequest request,
        io.grpc.stub.StreamObserver<nodepool.ReportStatusResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getReportStatusMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void returnTaskResult(nodepool.ReturnTaskResultRequest request,
        io.grpc.stub.StreamObserver<nodepool.ReturnTaskResultResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getReturnTaskResultMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void taskCompleted(nodepool.TaskCompletedRequest request,
        io.grpc.stub.StreamObserver<nodepool.TaskCompletedResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getTaskCompletedMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   */
  public static final class NodepoolBlockingStub extends io.grpc.stub.AbstractStub<NodepoolBlockingStub> {
    private NodepoolBlockingStub(io.grpc.Channel channel) {
      super(channel);
    }

    private NodepoolBlockingStub(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected NodepoolBlockingStub build(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      return new NodepoolBlockingStub(channel, callOptions);
    }

    /**
     */
    public nodepool.LoginResponse login(nodepool.LoginRequest request) {
      return blockingUnaryCall(
          getChannel(), getLoginMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.RegisterWorkerNodeResponse registerWorkerNode(nodepool.RegisterWorkerNodeRequest request) {
      return blockingUnaryCall(
          getChannel(), getRegisterWorkerNodeMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.ReportStatusResponse reportStatus(nodepool.ReportStatusRequest request) {
      return blockingUnaryCall(
          getChannel(), getReportStatusMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.ReturnTaskResultResponse returnTaskResult(nodepool.ReturnTaskResultRequest request) {
      return blockingUnaryCall(
          getChannel(), getReturnTaskResultMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.TaskCompletedResponse taskCompleted(nodepool.TaskCompletedRequest request) {
      return blockingUnaryCall(
          getChannel(), getTaskCompletedMethod(), getCallOptions(), request);
    }
  }

  /**
   */
  public static final class NodepoolFutureStub extends io.grpc.stub.AbstractStub<NodepoolFutureStub> {
    private NodepoolFutureStub(io.grpc.Channel channel) {
      super(channel);
    }

    private NodepoolFutureStub(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected NodepoolFutureStub build(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      return new NodepoolFutureStub(channel, callOptions);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.LoginResponse> login(
        nodepool.LoginRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getLoginMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.RegisterWorkerNodeResponse> registerWorkerNode(
        nodepool.RegisterWorkerNodeRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getRegisterWorkerNodeMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.ReportStatusResponse> reportStatus(
        nodepool.ReportStatusRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getReportStatusMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.ReturnTaskResultResponse> returnTaskResult(
        nodepool.ReturnTaskResultRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getReturnTaskResultMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.TaskCompletedResponse> taskCompleted(
        nodepool.TaskCompletedRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getTaskCompletedMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_LOGIN = 0;
  private static final int METHODID_REGISTER_WORKER_NODE = 1;
  private static final int METHODID_REPORT_STATUS = 2;
  private static final int METHODID_RETURN_TASK_RESULT = 3;
  private static final int METHODID_TASK_COMPLETED = 4;

  private static final class MethodHandlers<Req, Resp> implements
      io.grpc.stub.ServerCalls.UnaryMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ServerStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ClientStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.BidiStreamingMethod<Req, Resp> {
    private final NodepoolImplBase serviceImpl;
    private final int methodId;

    MethodHandlers(NodepoolImplBase serviceImpl, int methodId) {
      this.serviceImpl = serviceImpl;
      this.methodId = methodId;
    }

    @java.lang.Override
    @java.lang.SuppressWarnings("unchecked")
    public void invoke(Req request, io.grpc.stub.StreamObserver<Resp> responseObserver) {
      switch (methodId) {
        case METHODID_LOGIN:
          serviceImpl.login((nodepool.LoginRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.LoginResponse>) responseObserver);
          break;
        case METHODID_REGISTER_WORKER_NODE:
          serviceImpl.registerWorkerNode((nodepool.RegisterWorkerNodeRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.RegisterWorkerNodeResponse>) responseObserver);
          break;
        case METHODID_REPORT_STATUS:
          serviceImpl.reportStatus((nodepool.ReportStatusRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.ReportStatusResponse>) responseObserver);
          break;
        case METHODID_RETURN_TASK_RESULT:
          serviceImpl.returnTaskResult((nodepool.ReturnTaskResultRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.ReturnTaskResultResponse>) responseObserver);
          break;
        case METHODID_TASK_COMPLETED:
          serviceImpl.taskCompleted((nodepool.TaskCompletedRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.TaskCompletedResponse>) responseObserver);
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

  private static abstract class NodepoolBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    NodepoolBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return nodepool.NodepoolOuterClass.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("Nodepool");
    }
  }

  private static final class NodepoolFileDescriptorSupplier
      extends NodepoolBaseDescriptorSupplier {
    NodepoolFileDescriptorSupplier() {}
  }

  private static final class NodepoolMethodDescriptorSupplier
      extends NodepoolBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final String methodName;

    NodepoolMethodDescriptorSupplier(String methodName) {
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
      synchronized (NodepoolGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new NodepoolFileDescriptorSupplier())
              .addMethod(getLoginMethod())
              .addMethod(getRegisterWorkerNodeMethod())
              .addMethod(getReportStatusMethod())
              .addMethod(getReturnTaskResultMethod())
              .addMethod(getTaskCompletedMethod())
              .build();
        }
      }
    }
    return result;
  }
}
