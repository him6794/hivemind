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
public final class WorkerNodeServiceGrpc {

  private WorkerNodeServiceGrpc() {}

  public static final String SERVICE_NAME = "nodepool.WorkerNodeService";

  // Static method descriptors that strictly reflect the proto.
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getExecuteTaskMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.ExecuteTaskRequest,
      nodepool.ExecuteTaskResponse> METHOD_EXECUTE_TASK = getExecuteTaskMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.ExecuteTaskRequest,
      nodepool.ExecuteTaskResponse> getExecuteTaskMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.ExecuteTaskRequest,
      nodepool.ExecuteTaskResponse> getExecuteTaskMethod() {
    io.grpc.MethodDescriptor<nodepool.ExecuteTaskRequest, nodepool.ExecuteTaskResponse> getExecuteTaskMethod;
    if ((getExecuteTaskMethod = WorkerNodeServiceGrpc.getExecuteTaskMethod) == null) {
      synchronized (WorkerNodeServiceGrpc.class) {
        if ((getExecuteTaskMethod = WorkerNodeServiceGrpc.getExecuteTaskMethod) == null) {
          WorkerNodeServiceGrpc.getExecuteTaskMethod = getExecuteTaskMethod = 
              io.grpc.MethodDescriptor.<nodepool.ExecuteTaskRequest, nodepool.ExecuteTaskResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.WorkerNodeService", "ExecuteTask"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.ExecuteTaskRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.ExecuteTaskResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new WorkerNodeServiceMethodDescriptorSupplier("ExecuteTask"))
                  .build();
          }
        }
     }
     return getExecuteTaskMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getReportOutputMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.ReportOutputRequest,
      nodepool.StatusResponse> METHOD_REPORT_OUTPUT = getReportOutputMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.ReportOutputRequest,
      nodepool.StatusResponse> getReportOutputMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.ReportOutputRequest,
      nodepool.StatusResponse> getReportOutputMethod() {
    io.grpc.MethodDescriptor<nodepool.ReportOutputRequest, nodepool.StatusResponse> getReportOutputMethod;
    if ((getReportOutputMethod = WorkerNodeServiceGrpc.getReportOutputMethod) == null) {
      synchronized (WorkerNodeServiceGrpc.class) {
        if ((getReportOutputMethod = WorkerNodeServiceGrpc.getReportOutputMethod) == null) {
          WorkerNodeServiceGrpc.getReportOutputMethod = getReportOutputMethod = 
              io.grpc.MethodDescriptor.<nodepool.ReportOutputRequest, nodepool.StatusResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.WorkerNodeService", "ReportOutput"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.ReportOutputRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StatusResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new WorkerNodeServiceMethodDescriptorSupplier("ReportOutput"))
                  .build();
          }
        }
     }
     return getReportOutputMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getReportRunningStatusMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.RunningStatusRequest,
      nodepool.RunningStatusResponse> METHOD_REPORT_RUNNING_STATUS = getReportRunningStatusMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.RunningStatusRequest,
      nodepool.RunningStatusResponse> getReportRunningStatusMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.RunningStatusRequest,
      nodepool.RunningStatusResponse> getReportRunningStatusMethod() {
    io.grpc.MethodDescriptor<nodepool.RunningStatusRequest, nodepool.RunningStatusResponse> getReportRunningStatusMethod;
    if ((getReportRunningStatusMethod = WorkerNodeServiceGrpc.getReportRunningStatusMethod) == null) {
      synchronized (WorkerNodeServiceGrpc.class) {
        if ((getReportRunningStatusMethod = WorkerNodeServiceGrpc.getReportRunningStatusMethod) == null) {
          WorkerNodeServiceGrpc.getReportRunningStatusMethod = getReportRunningStatusMethod = 
              io.grpc.MethodDescriptor.<nodepool.RunningStatusRequest, nodepool.RunningStatusResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.WorkerNodeService", "ReportRunningStatus"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.RunningStatusRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.RunningStatusResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new WorkerNodeServiceMethodDescriptorSupplier("ReportRunningStatus"))
                  .build();
          }
        }
     }
     return getReportRunningStatusMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getStopTaskExecutionMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.StopTaskExecutionRequest,
      nodepool.StopTaskExecutionResponse> METHOD_STOP_TASK_EXECUTION = getStopTaskExecutionMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.StopTaskExecutionRequest,
      nodepool.StopTaskExecutionResponse> getStopTaskExecutionMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.StopTaskExecutionRequest,
      nodepool.StopTaskExecutionResponse> getStopTaskExecutionMethod() {
    io.grpc.MethodDescriptor<nodepool.StopTaskExecutionRequest, nodepool.StopTaskExecutionResponse> getStopTaskExecutionMethod;
    if ((getStopTaskExecutionMethod = WorkerNodeServiceGrpc.getStopTaskExecutionMethod) == null) {
      synchronized (WorkerNodeServiceGrpc.class) {
        if ((getStopTaskExecutionMethod = WorkerNodeServiceGrpc.getStopTaskExecutionMethod) == null) {
          WorkerNodeServiceGrpc.getStopTaskExecutionMethod = getStopTaskExecutionMethod = 
              io.grpc.MethodDescriptor.<nodepool.StopTaskExecutionRequest, nodepool.StopTaskExecutionResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.WorkerNodeService", "StopTaskExecution"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StopTaskExecutionRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StopTaskExecutionResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new WorkerNodeServiceMethodDescriptorSupplier("StopTaskExecution"))
                  .build();
          }
        }
     }
     return getStopTaskExecutionMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static WorkerNodeServiceStub newStub(io.grpc.Channel channel) {
    return new WorkerNodeServiceStub(channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static WorkerNodeServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    return new WorkerNodeServiceBlockingStub(channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static WorkerNodeServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    return new WorkerNodeServiceFutureStub(channel);
  }

  /**
   */
  public static abstract class WorkerNodeServiceImplBase implements io.grpc.BindableService {

    /**
     */
    public void executeTask(nodepool.ExecuteTaskRequest request,
        io.grpc.stub.StreamObserver<nodepool.ExecuteTaskResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getExecuteTaskMethod(), responseObserver);
    }

    /**
     */
    public void reportOutput(nodepool.ReportOutputRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getReportOutputMethod(), responseObserver);
    }

    /**
     */
    public void reportRunningStatus(nodepool.RunningStatusRequest request,
        io.grpc.stub.StreamObserver<nodepool.RunningStatusResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getReportRunningStatusMethod(), responseObserver);
    }

    /**
     */
    public void stopTaskExecution(nodepool.StopTaskExecutionRequest request,
        io.grpc.stub.StreamObserver<nodepool.StopTaskExecutionResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getStopTaskExecutionMethod(), responseObserver);
    }

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return io.grpc.ServerServiceDefinition.builder(getServiceDescriptor())
          .addMethod(
            getExecuteTaskMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.ExecuteTaskRequest,
                nodepool.ExecuteTaskResponse>(
                  this, METHODID_EXECUTE_TASK)))
          .addMethod(
            getReportOutputMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.ReportOutputRequest,
                nodepool.StatusResponse>(
                  this, METHODID_REPORT_OUTPUT)))
          .addMethod(
            getReportRunningStatusMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.RunningStatusRequest,
                nodepool.RunningStatusResponse>(
                  this, METHODID_REPORT_RUNNING_STATUS)))
          .addMethod(
            getStopTaskExecutionMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.StopTaskExecutionRequest,
                nodepool.StopTaskExecutionResponse>(
                  this, METHODID_STOP_TASK_EXECUTION)))
          .build();
    }
  }

  /**
   */
  public static final class WorkerNodeServiceStub extends io.grpc.stub.AbstractStub<WorkerNodeServiceStub> {
    private WorkerNodeServiceStub(io.grpc.Channel channel) {
      super(channel);
    }

    private WorkerNodeServiceStub(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected WorkerNodeServiceStub build(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      return new WorkerNodeServiceStub(channel, callOptions);
    }

    /**
     */
    public void executeTask(nodepool.ExecuteTaskRequest request,
        io.grpc.stub.StreamObserver<nodepool.ExecuteTaskResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getExecuteTaskMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void reportOutput(nodepool.ReportOutputRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getReportOutputMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void reportRunningStatus(nodepool.RunningStatusRequest request,
        io.grpc.stub.StreamObserver<nodepool.RunningStatusResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getReportRunningStatusMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void stopTaskExecution(nodepool.StopTaskExecutionRequest request,
        io.grpc.stub.StreamObserver<nodepool.StopTaskExecutionResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getStopTaskExecutionMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   */
  public static final class WorkerNodeServiceBlockingStub extends io.grpc.stub.AbstractStub<WorkerNodeServiceBlockingStub> {
    private WorkerNodeServiceBlockingStub(io.grpc.Channel channel) {
      super(channel);
    }

    private WorkerNodeServiceBlockingStub(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected WorkerNodeServiceBlockingStub build(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      return new WorkerNodeServiceBlockingStub(channel, callOptions);
    }

    /**
     */
    public nodepool.ExecuteTaskResponse executeTask(nodepool.ExecuteTaskRequest request) {
      return blockingUnaryCall(
          getChannel(), getExecuteTaskMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.StatusResponse reportOutput(nodepool.ReportOutputRequest request) {
      return blockingUnaryCall(
          getChannel(), getReportOutputMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.RunningStatusResponse reportRunningStatus(nodepool.RunningStatusRequest request) {
      return blockingUnaryCall(
          getChannel(), getReportRunningStatusMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.StopTaskExecutionResponse stopTaskExecution(nodepool.StopTaskExecutionRequest request) {
      return blockingUnaryCall(
          getChannel(), getStopTaskExecutionMethod(), getCallOptions(), request);
    }
  }

  /**
   */
  public static final class WorkerNodeServiceFutureStub extends io.grpc.stub.AbstractStub<WorkerNodeServiceFutureStub> {
    private WorkerNodeServiceFutureStub(io.grpc.Channel channel) {
      super(channel);
    }

    private WorkerNodeServiceFutureStub(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected WorkerNodeServiceFutureStub build(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      return new WorkerNodeServiceFutureStub(channel, callOptions);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.ExecuteTaskResponse> executeTask(
        nodepool.ExecuteTaskRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getExecuteTaskMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.StatusResponse> reportOutput(
        nodepool.ReportOutputRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getReportOutputMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.RunningStatusResponse> reportRunningStatus(
        nodepool.RunningStatusRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getReportRunningStatusMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.StopTaskExecutionResponse> stopTaskExecution(
        nodepool.StopTaskExecutionRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getStopTaskExecutionMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_EXECUTE_TASK = 0;
  private static final int METHODID_REPORT_OUTPUT = 1;
  private static final int METHODID_REPORT_RUNNING_STATUS = 2;
  private static final int METHODID_STOP_TASK_EXECUTION = 3;

  private static final class MethodHandlers<Req, Resp> implements
      io.grpc.stub.ServerCalls.UnaryMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ServerStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ClientStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.BidiStreamingMethod<Req, Resp> {
    private final WorkerNodeServiceImplBase serviceImpl;
    private final int methodId;

    MethodHandlers(WorkerNodeServiceImplBase serviceImpl, int methodId) {
      this.serviceImpl = serviceImpl;
      this.methodId = methodId;
    }

    @java.lang.Override
    @java.lang.SuppressWarnings("unchecked")
    public void invoke(Req request, io.grpc.stub.StreamObserver<Resp> responseObserver) {
      switch (methodId) {
        case METHODID_EXECUTE_TASK:
          serviceImpl.executeTask((nodepool.ExecuteTaskRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.ExecuteTaskResponse>) responseObserver);
          break;
        case METHODID_REPORT_OUTPUT:
          serviceImpl.reportOutput((nodepool.ReportOutputRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.StatusResponse>) responseObserver);
          break;
        case METHODID_REPORT_RUNNING_STATUS:
          serviceImpl.reportRunningStatus((nodepool.RunningStatusRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.RunningStatusResponse>) responseObserver);
          break;
        case METHODID_STOP_TASK_EXECUTION:
          serviceImpl.stopTaskExecution((nodepool.StopTaskExecutionRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.StopTaskExecutionResponse>) responseObserver);
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

  private static abstract class WorkerNodeServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    WorkerNodeServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return nodepool.NodepoolOuterClass.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("WorkerNodeService");
    }
  }

  private static final class WorkerNodeServiceFileDescriptorSupplier
      extends WorkerNodeServiceBaseDescriptorSupplier {
    WorkerNodeServiceFileDescriptorSupplier() {}
  }

  private static final class WorkerNodeServiceMethodDescriptorSupplier
      extends WorkerNodeServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final String methodName;

    WorkerNodeServiceMethodDescriptorSupplier(String methodName) {
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
      synchronized (WorkerNodeServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new WorkerNodeServiceFileDescriptorSupplier())
              .addMethod(getExecuteTaskMethod())
              .addMethod(getReportOutputMethod())
              .addMethod(getReportRunningStatusMethod())
              .addMethod(getStopTaskExecutionMethod())
              .build();
        }
      }
    }
    return result;
  }
}
