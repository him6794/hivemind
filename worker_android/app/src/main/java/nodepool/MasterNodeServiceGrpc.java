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
public final class MasterNodeServiceGrpc {

  private MasterNodeServiceGrpc() {}

  public static final String SERVICE_NAME = "nodepool.MasterNodeService";

  // Static method descriptors that strictly reflect the proto.
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getUploadTaskMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.UploadTaskRequest,
      nodepool.UploadTaskResponse> METHOD_UPLOAD_TASK = getUploadTaskMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.UploadTaskRequest,
      nodepool.UploadTaskResponse> getUploadTaskMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.UploadTaskRequest,
      nodepool.UploadTaskResponse> getUploadTaskMethod() {
    io.grpc.MethodDescriptor<nodepool.UploadTaskRequest, nodepool.UploadTaskResponse> getUploadTaskMethod;
    if ((getUploadTaskMethod = MasterNodeServiceGrpc.getUploadTaskMethod) == null) {
      synchronized (MasterNodeServiceGrpc.class) {
        if ((getUploadTaskMethod = MasterNodeServiceGrpc.getUploadTaskMethod) == null) {
          MasterNodeServiceGrpc.getUploadTaskMethod = getUploadTaskMethod = 
              io.grpc.MethodDescriptor.<nodepool.UploadTaskRequest, nodepool.UploadTaskResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.MasterNodeService", "UploadTask"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.UploadTaskRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.UploadTaskResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new MasterNodeServiceMethodDescriptorSupplier("UploadTask"))
                  .build();
          }
        }
     }
     return getUploadTaskMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getPollTaskStatusMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.PollTaskStatusRequest,
      nodepool.PollTaskStatusResponse> METHOD_POLL_TASK_STATUS = getPollTaskStatusMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.PollTaskStatusRequest,
      nodepool.PollTaskStatusResponse> getPollTaskStatusMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.PollTaskStatusRequest,
      nodepool.PollTaskStatusResponse> getPollTaskStatusMethod() {
    io.grpc.MethodDescriptor<nodepool.PollTaskStatusRequest, nodepool.PollTaskStatusResponse> getPollTaskStatusMethod;
    if ((getPollTaskStatusMethod = MasterNodeServiceGrpc.getPollTaskStatusMethod) == null) {
      synchronized (MasterNodeServiceGrpc.class) {
        if ((getPollTaskStatusMethod = MasterNodeServiceGrpc.getPollTaskStatusMethod) == null) {
          MasterNodeServiceGrpc.getPollTaskStatusMethod = getPollTaskStatusMethod = 
              io.grpc.MethodDescriptor.<nodepool.PollTaskStatusRequest, nodepool.PollTaskStatusResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.MasterNodeService", "PollTaskStatus"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.PollTaskStatusRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.PollTaskStatusResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new MasterNodeServiceMethodDescriptorSupplier("PollTaskStatus"))
                  .build();
          }
        }
     }
     return getPollTaskStatusMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getStoreOutputMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.StoreOutputRequest,
      nodepool.StatusResponse> METHOD_STORE_OUTPUT = getStoreOutputMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.StoreOutputRequest,
      nodepool.StatusResponse> getStoreOutputMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.StoreOutputRequest,
      nodepool.StatusResponse> getStoreOutputMethod() {
    io.grpc.MethodDescriptor<nodepool.StoreOutputRequest, nodepool.StatusResponse> getStoreOutputMethod;
    if ((getStoreOutputMethod = MasterNodeServiceGrpc.getStoreOutputMethod) == null) {
      synchronized (MasterNodeServiceGrpc.class) {
        if ((getStoreOutputMethod = MasterNodeServiceGrpc.getStoreOutputMethod) == null) {
          MasterNodeServiceGrpc.getStoreOutputMethod = getStoreOutputMethod = 
              io.grpc.MethodDescriptor.<nodepool.StoreOutputRequest, nodepool.StatusResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.MasterNodeService", "StoreOutput"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StoreOutputRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StatusResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new MasterNodeServiceMethodDescriptorSupplier("StoreOutput"))
                  .build();
          }
        }
     }
     return getStoreOutputMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getStoreResultMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.StoreResultRequest,
      nodepool.StatusResponse> METHOD_STORE_RESULT = getStoreResultMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.StoreResultRequest,
      nodepool.StatusResponse> getStoreResultMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.StoreResultRequest,
      nodepool.StatusResponse> getStoreResultMethod() {
    io.grpc.MethodDescriptor<nodepool.StoreResultRequest, nodepool.StatusResponse> getStoreResultMethod;
    if ((getStoreResultMethod = MasterNodeServiceGrpc.getStoreResultMethod) == null) {
      synchronized (MasterNodeServiceGrpc.class) {
        if ((getStoreResultMethod = MasterNodeServiceGrpc.getStoreResultMethod) == null) {
          MasterNodeServiceGrpc.getStoreResultMethod = getStoreResultMethod = 
              io.grpc.MethodDescriptor.<nodepool.StoreResultRequest, nodepool.StatusResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.MasterNodeService", "StoreResult"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StoreResultRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StatusResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new MasterNodeServiceMethodDescriptorSupplier("StoreResult"))
                  .build();
          }
        }
     }
     return getStoreResultMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getGetTaskResultMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.GetTaskResultRequest,
      nodepool.GetTaskResultResponse> METHOD_GET_TASK_RESULT = getGetTaskResultMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.GetTaskResultRequest,
      nodepool.GetTaskResultResponse> getGetTaskResultMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.GetTaskResultRequest,
      nodepool.GetTaskResultResponse> getGetTaskResultMethod() {
    io.grpc.MethodDescriptor<nodepool.GetTaskResultRequest, nodepool.GetTaskResultResponse> getGetTaskResultMethod;
    if ((getGetTaskResultMethod = MasterNodeServiceGrpc.getGetTaskResultMethod) == null) {
      synchronized (MasterNodeServiceGrpc.class) {
        if ((getGetTaskResultMethod = MasterNodeServiceGrpc.getGetTaskResultMethod) == null) {
          MasterNodeServiceGrpc.getGetTaskResultMethod = getGetTaskResultMethod = 
              io.grpc.MethodDescriptor.<nodepool.GetTaskResultRequest, nodepool.GetTaskResultResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.MasterNodeService", "GetTaskResult"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.GetTaskResultRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.GetTaskResultResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new MasterNodeServiceMethodDescriptorSupplier("GetTaskResult"))
                  .build();
          }
        }
     }
     return getGetTaskResultMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getTaskCompletedMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.TaskCompletedRequest,
      nodepool.StatusResponse> METHOD_TASK_COMPLETED = getTaskCompletedMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.TaskCompletedRequest,
      nodepool.StatusResponse> getTaskCompletedMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.TaskCompletedRequest,
      nodepool.StatusResponse> getTaskCompletedMethod() {
    io.grpc.MethodDescriptor<nodepool.TaskCompletedRequest, nodepool.StatusResponse> getTaskCompletedMethod;
    if ((getTaskCompletedMethod = MasterNodeServiceGrpc.getTaskCompletedMethod) == null) {
      synchronized (MasterNodeServiceGrpc.class) {
        if ((getTaskCompletedMethod = MasterNodeServiceGrpc.getTaskCompletedMethod) == null) {
          MasterNodeServiceGrpc.getTaskCompletedMethod = getTaskCompletedMethod = 
              io.grpc.MethodDescriptor.<nodepool.TaskCompletedRequest, nodepool.StatusResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.MasterNodeService", "TaskCompleted"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.TaskCompletedRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StatusResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new MasterNodeServiceMethodDescriptorSupplier("TaskCompleted"))
                  .build();
          }
        }
     }
     return getTaskCompletedMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getStoreLogsMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.StoreLogsRequest,
      nodepool.StatusResponse> METHOD_STORE_LOGS = getStoreLogsMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.StoreLogsRequest,
      nodepool.StatusResponse> getStoreLogsMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.StoreLogsRequest,
      nodepool.StatusResponse> getStoreLogsMethod() {
    io.grpc.MethodDescriptor<nodepool.StoreLogsRequest, nodepool.StatusResponse> getStoreLogsMethod;
    if ((getStoreLogsMethod = MasterNodeServiceGrpc.getStoreLogsMethod) == null) {
      synchronized (MasterNodeServiceGrpc.class) {
        if ((getStoreLogsMethod = MasterNodeServiceGrpc.getStoreLogsMethod) == null) {
          MasterNodeServiceGrpc.getStoreLogsMethod = getStoreLogsMethod = 
              io.grpc.MethodDescriptor.<nodepool.StoreLogsRequest, nodepool.StatusResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.MasterNodeService", "StoreLogs"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StoreLogsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StatusResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new MasterNodeServiceMethodDescriptorSupplier("StoreLogs"))
                  .build();
          }
        }
     }
     return getStoreLogsMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getGetTaskLogsMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.GetTaskLogsRequest,
      nodepool.GetTaskLogsResponse> METHOD_GET_TASK_LOGS = getGetTaskLogsMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.GetTaskLogsRequest,
      nodepool.GetTaskLogsResponse> getGetTaskLogsMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.GetTaskLogsRequest,
      nodepool.GetTaskLogsResponse> getGetTaskLogsMethod() {
    io.grpc.MethodDescriptor<nodepool.GetTaskLogsRequest, nodepool.GetTaskLogsResponse> getGetTaskLogsMethod;
    if ((getGetTaskLogsMethod = MasterNodeServiceGrpc.getGetTaskLogsMethod) == null) {
      synchronized (MasterNodeServiceGrpc.class) {
        if ((getGetTaskLogsMethod = MasterNodeServiceGrpc.getGetTaskLogsMethod) == null) {
          MasterNodeServiceGrpc.getGetTaskLogsMethod = getGetTaskLogsMethod = 
              io.grpc.MethodDescriptor.<nodepool.GetTaskLogsRequest, nodepool.GetTaskLogsResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.MasterNodeService", "GetTaskLogs"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.GetTaskLogsRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.GetTaskLogsResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new MasterNodeServiceMethodDescriptorSupplier("GetTaskLogs"))
                  .build();
          }
        }
     }
     return getGetTaskLogsMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getGetAllTasksMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.GetAllTasksRequest,
      nodepool.GetAllTasksResponse> METHOD_GET_ALL_TASKS = getGetAllTasksMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.GetAllTasksRequest,
      nodepool.GetAllTasksResponse> getGetAllTasksMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.GetAllTasksRequest,
      nodepool.GetAllTasksResponse> getGetAllTasksMethod() {
    io.grpc.MethodDescriptor<nodepool.GetAllTasksRequest, nodepool.GetAllTasksResponse> getGetAllTasksMethod;
    if ((getGetAllTasksMethod = MasterNodeServiceGrpc.getGetAllTasksMethod) == null) {
      synchronized (MasterNodeServiceGrpc.class) {
        if ((getGetAllTasksMethod = MasterNodeServiceGrpc.getGetAllTasksMethod) == null) {
          MasterNodeServiceGrpc.getGetAllTasksMethod = getGetAllTasksMethod = 
              io.grpc.MethodDescriptor.<nodepool.GetAllTasksRequest, nodepool.GetAllTasksResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.MasterNodeService", "GetAllTasks"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.GetAllTasksRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.GetAllTasksResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new MasterNodeServiceMethodDescriptorSupplier("GetAllTasks"))
                  .build();
          }
        }
     }
     return getGetAllTasksMethod;
  }
  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  @java.lang.Deprecated // Use {@link #getStopTaskMethod()} instead. 
  public static final io.grpc.MethodDescriptor<nodepool.StopTaskRequest,
      nodepool.StopTaskResponse> METHOD_STOP_TASK = getStopTaskMethod();

  private static volatile io.grpc.MethodDescriptor<nodepool.StopTaskRequest,
      nodepool.StopTaskResponse> getStopTaskMethod;

  @io.grpc.ExperimentalApi("https://github.com/grpc/grpc-java/issues/1901")
  public static io.grpc.MethodDescriptor<nodepool.StopTaskRequest,
      nodepool.StopTaskResponse> getStopTaskMethod() {
    io.grpc.MethodDescriptor<nodepool.StopTaskRequest, nodepool.StopTaskResponse> getStopTaskMethod;
    if ((getStopTaskMethod = MasterNodeServiceGrpc.getStopTaskMethod) == null) {
      synchronized (MasterNodeServiceGrpc.class) {
        if ((getStopTaskMethod = MasterNodeServiceGrpc.getStopTaskMethod) == null) {
          MasterNodeServiceGrpc.getStopTaskMethod = getStopTaskMethod = 
              io.grpc.MethodDescriptor.<nodepool.StopTaskRequest, nodepool.StopTaskResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.MasterNodeService", "StopTask"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StopTaskRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.StopTaskResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new MasterNodeServiceMethodDescriptorSupplier("StopTask"))
                  .build();
          }
        }
     }
     return getStopTaskMethod;
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
    if ((getReturnTaskResultMethod = MasterNodeServiceGrpc.getReturnTaskResultMethod) == null) {
      synchronized (MasterNodeServiceGrpc.class) {
        if ((getReturnTaskResultMethod = MasterNodeServiceGrpc.getReturnTaskResultMethod) == null) {
          MasterNodeServiceGrpc.getReturnTaskResultMethod = getReturnTaskResultMethod = 
              io.grpc.MethodDescriptor.<nodepool.ReturnTaskResultRequest, nodepool.ReturnTaskResultResponse>newBuilder()
              .setType(io.grpc.MethodDescriptor.MethodType.UNARY)
              .setFullMethodName(generateFullMethodName(
                  "nodepool.MasterNodeService", "ReturnTaskResult"))
              .setSampledToLocalTracing(true)
              .setRequestMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.ReturnTaskResultRequest.getDefaultInstance()))
              .setResponseMarshaller(io.grpc.protobuf.ProtoUtils.marshaller(
                  nodepool.ReturnTaskResultResponse.getDefaultInstance()))
                  .setSchemaDescriptor(new MasterNodeServiceMethodDescriptorSupplier("ReturnTaskResult"))
                  .build();
          }
        }
     }
     return getReturnTaskResultMethod;
  }

  /**
   * Creates a new async stub that supports all call types for the service
   */
  public static MasterNodeServiceStub newStub(io.grpc.Channel channel) {
    return new MasterNodeServiceStub(channel);
  }

  /**
   * Creates a new blocking-style stub that supports unary and streaming output calls on the service
   */
  public static MasterNodeServiceBlockingStub newBlockingStub(
      io.grpc.Channel channel) {
    return new MasterNodeServiceBlockingStub(channel);
  }

  /**
   * Creates a new ListenableFuture-style stub that supports unary calls on the service
   */
  public static MasterNodeServiceFutureStub newFutureStub(
      io.grpc.Channel channel) {
    return new MasterNodeServiceFutureStub(channel);
  }

  /**
   */
  public static abstract class MasterNodeServiceImplBase implements io.grpc.BindableService {

    /**
     */
    public void uploadTask(nodepool.UploadTaskRequest request,
        io.grpc.stub.StreamObserver<nodepool.UploadTaskResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getUploadTaskMethod(), responseObserver);
    }

    /**
     */
    public void pollTaskStatus(nodepool.PollTaskStatusRequest request,
        io.grpc.stub.StreamObserver<nodepool.PollTaskStatusResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getPollTaskStatusMethod(), responseObserver);
    }

    /**
     */
    public void storeOutput(nodepool.StoreOutputRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getStoreOutputMethod(), responseObserver);
    }

    /**
     */
    public void storeResult(nodepool.StoreResultRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getStoreResultMethod(), responseObserver);
    }

    /**
     */
    public void getTaskResult(nodepool.GetTaskResultRequest request,
        io.grpc.stub.StreamObserver<nodepool.GetTaskResultResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getGetTaskResultMethod(), responseObserver);
    }

    /**
     */
    public void taskCompleted(nodepool.TaskCompletedRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getTaskCompletedMethod(), responseObserver);
    }

    /**
     */
    public void storeLogs(nodepool.StoreLogsRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getStoreLogsMethod(), responseObserver);
    }

    /**
     */
    public void getTaskLogs(nodepool.GetTaskLogsRequest request,
        io.grpc.stub.StreamObserver<nodepool.GetTaskLogsResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getGetTaskLogsMethod(), responseObserver);
    }

    /**
     */
    public void getAllTasks(nodepool.GetAllTasksRequest request,
        io.grpc.stub.StreamObserver<nodepool.GetAllTasksResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getGetAllTasksMethod(), responseObserver);
    }

    /**
     */
    public void stopTask(nodepool.StopTaskRequest request,
        io.grpc.stub.StreamObserver<nodepool.StopTaskResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getStopTaskMethod(), responseObserver);
    }

    /**
     */
    public void returnTaskResult(nodepool.ReturnTaskResultRequest request,
        io.grpc.stub.StreamObserver<nodepool.ReturnTaskResultResponse> responseObserver) {
      asyncUnimplementedUnaryCall(getReturnTaskResultMethod(), responseObserver);
    }

    @java.lang.Override public final io.grpc.ServerServiceDefinition bindService() {
      return io.grpc.ServerServiceDefinition.builder(getServiceDescriptor())
          .addMethod(
            getUploadTaskMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.UploadTaskRequest,
                nodepool.UploadTaskResponse>(
                  this, METHODID_UPLOAD_TASK)))
          .addMethod(
            getPollTaskStatusMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.PollTaskStatusRequest,
                nodepool.PollTaskStatusResponse>(
                  this, METHODID_POLL_TASK_STATUS)))
          .addMethod(
            getStoreOutputMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.StoreOutputRequest,
                nodepool.StatusResponse>(
                  this, METHODID_STORE_OUTPUT)))
          .addMethod(
            getStoreResultMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.StoreResultRequest,
                nodepool.StatusResponse>(
                  this, METHODID_STORE_RESULT)))
          .addMethod(
            getGetTaskResultMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.GetTaskResultRequest,
                nodepool.GetTaskResultResponse>(
                  this, METHODID_GET_TASK_RESULT)))
          .addMethod(
            getTaskCompletedMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.TaskCompletedRequest,
                nodepool.StatusResponse>(
                  this, METHODID_TASK_COMPLETED)))
          .addMethod(
            getStoreLogsMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.StoreLogsRequest,
                nodepool.StatusResponse>(
                  this, METHODID_STORE_LOGS)))
          .addMethod(
            getGetTaskLogsMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.GetTaskLogsRequest,
                nodepool.GetTaskLogsResponse>(
                  this, METHODID_GET_TASK_LOGS)))
          .addMethod(
            getGetAllTasksMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.GetAllTasksRequest,
                nodepool.GetAllTasksResponse>(
                  this, METHODID_GET_ALL_TASKS)))
          .addMethod(
            getStopTaskMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.StopTaskRequest,
                nodepool.StopTaskResponse>(
                  this, METHODID_STOP_TASK)))
          .addMethod(
            getReturnTaskResultMethod(),
            asyncUnaryCall(
              new MethodHandlers<
                nodepool.ReturnTaskResultRequest,
                nodepool.ReturnTaskResultResponse>(
                  this, METHODID_RETURN_TASK_RESULT)))
          .build();
    }
  }

  /**
   */
  public static final class MasterNodeServiceStub extends io.grpc.stub.AbstractStub<MasterNodeServiceStub> {
    private MasterNodeServiceStub(io.grpc.Channel channel) {
      super(channel);
    }

    private MasterNodeServiceStub(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected MasterNodeServiceStub build(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      return new MasterNodeServiceStub(channel, callOptions);
    }

    /**
     */
    public void uploadTask(nodepool.UploadTaskRequest request,
        io.grpc.stub.StreamObserver<nodepool.UploadTaskResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getUploadTaskMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void pollTaskStatus(nodepool.PollTaskStatusRequest request,
        io.grpc.stub.StreamObserver<nodepool.PollTaskStatusResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getPollTaskStatusMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void storeOutput(nodepool.StoreOutputRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getStoreOutputMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void storeResult(nodepool.StoreResultRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getStoreResultMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getTaskResult(nodepool.GetTaskResultRequest request,
        io.grpc.stub.StreamObserver<nodepool.GetTaskResultResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getGetTaskResultMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void taskCompleted(nodepool.TaskCompletedRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getTaskCompletedMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void storeLogs(nodepool.StoreLogsRequest request,
        io.grpc.stub.StreamObserver<nodepool.StatusResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getStoreLogsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getTaskLogs(nodepool.GetTaskLogsRequest request,
        io.grpc.stub.StreamObserver<nodepool.GetTaskLogsResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getGetTaskLogsMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void getAllTasks(nodepool.GetAllTasksRequest request,
        io.grpc.stub.StreamObserver<nodepool.GetAllTasksResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getGetAllTasksMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void stopTask(nodepool.StopTaskRequest request,
        io.grpc.stub.StreamObserver<nodepool.StopTaskResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getStopTaskMethod(), getCallOptions()), request, responseObserver);
    }

    /**
     */
    public void returnTaskResult(nodepool.ReturnTaskResultRequest request,
        io.grpc.stub.StreamObserver<nodepool.ReturnTaskResultResponse> responseObserver) {
      asyncUnaryCall(
          getChannel().newCall(getReturnTaskResultMethod(), getCallOptions()), request, responseObserver);
    }
  }

  /**
   */
  public static final class MasterNodeServiceBlockingStub extends io.grpc.stub.AbstractStub<MasterNodeServiceBlockingStub> {
    private MasterNodeServiceBlockingStub(io.grpc.Channel channel) {
      super(channel);
    }

    private MasterNodeServiceBlockingStub(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected MasterNodeServiceBlockingStub build(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      return new MasterNodeServiceBlockingStub(channel, callOptions);
    }

    /**
     */
    public nodepool.UploadTaskResponse uploadTask(nodepool.UploadTaskRequest request) {
      return blockingUnaryCall(
          getChannel(), getUploadTaskMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.PollTaskStatusResponse pollTaskStatus(nodepool.PollTaskStatusRequest request) {
      return blockingUnaryCall(
          getChannel(), getPollTaskStatusMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.StatusResponse storeOutput(nodepool.StoreOutputRequest request) {
      return blockingUnaryCall(
          getChannel(), getStoreOutputMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.StatusResponse storeResult(nodepool.StoreResultRequest request) {
      return blockingUnaryCall(
          getChannel(), getStoreResultMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.GetTaskResultResponse getTaskResult(nodepool.GetTaskResultRequest request) {
      return blockingUnaryCall(
          getChannel(), getGetTaskResultMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.StatusResponse taskCompleted(nodepool.TaskCompletedRequest request) {
      return blockingUnaryCall(
          getChannel(), getTaskCompletedMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.StatusResponse storeLogs(nodepool.StoreLogsRequest request) {
      return blockingUnaryCall(
          getChannel(), getStoreLogsMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.GetTaskLogsResponse getTaskLogs(nodepool.GetTaskLogsRequest request) {
      return blockingUnaryCall(
          getChannel(), getGetTaskLogsMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.GetAllTasksResponse getAllTasks(nodepool.GetAllTasksRequest request) {
      return blockingUnaryCall(
          getChannel(), getGetAllTasksMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.StopTaskResponse stopTask(nodepool.StopTaskRequest request) {
      return blockingUnaryCall(
          getChannel(), getStopTaskMethod(), getCallOptions(), request);
    }

    /**
     */
    public nodepool.ReturnTaskResultResponse returnTaskResult(nodepool.ReturnTaskResultRequest request) {
      return blockingUnaryCall(
          getChannel(), getReturnTaskResultMethod(), getCallOptions(), request);
    }
  }

  /**
   */
  public static final class MasterNodeServiceFutureStub extends io.grpc.stub.AbstractStub<MasterNodeServiceFutureStub> {
    private MasterNodeServiceFutureStub(io.grpc.Channel channel) {
      super(channel);
    }

    private MasterNodeServiceFutureStub(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      super(channel, callOptions);
    }

    @java.lang.Override
    protected MasterNodeServiceFutureStub build(io.grpc.Channel channel,
        io.grpc.CallOptions callOptions) {
      return new MasterNodeServiceFutureStub(channel, callOptions);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.UploadTaskResponse> uploadTask(
        nodepool.UploadTaskRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getUploadTaskMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.PollTaskStatusResponse> pollTaskStatus(
        nodepool.PollTaskStatusRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getPollTaskStatusMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.StatusResponse> storeOutput(
        nodepool.StoreOutputRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getStoreOutputMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.StatusResponse> storeResult(
        nodepool.StoreResultRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getStoreResultMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.GetTaskResultResponse> getTaskResult(
        nodepool.GetTaskResultRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getGetTaskResultMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.StatusResponse> taskCompleted(
        nodepool.TaskCompletedRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getTaskCompletedMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.StatusResponse> storeLogs(
        nodepool.StoreLogsRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getStoreLogsMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.GetTaskLogsResponse> getTaskLogs(
        nodepool.GetTaskLogsRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getGetTaskLogsMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.GetAllTasksResponse> getAllTasks(
        nodepool.GetAllTasksRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getGetAllTasksMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.StopTaskResponse> stopTask(
        nodepool.StopTaskRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getStopTaskMethod(), getCallOptions()), request);
    }

    /**
     */
    public com.google.common.util.concurrent.ListenableFuture<nodepool.ReturnTaskResultResponse> returnTaskResult(
        nodepool.ReturnTaskResultRequest request) {
      return futureUnaryCall(
          getChannel().newCall(getReturnTaskResultMethod(), getCallOptions()), request);
    }
  }

  private static final int METHODID_UPLOAD_TASK = 0;
  private static final int METHODID_POLL_TASK_STATUS = 1;
  private static final int METHODID_STORE_OUTPUT = 2;
  private static final int METHODID_STORE_RESULT = 3;
  private static final int METHODID_GET_TASK_RESULT = 4;
  private static final int METHODID_TASK_COMPLETED = 5;
  private static final int METHODID_STORE_LOGS = 6;
  private static final int METHODID_GET_TASK_LOGS = 7;
  private static final int METHODID_GET_ALL_TASKS = 8;
  private static final int METHODID_STOP_TASK = 9;
  private static final int METHODID_RETURN_TASK_RESULT = 10;

  private static final class MethodHandlers<Req, Resp> implements
      io.grpc.stub.ServerCalls.UnaryMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ServerStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.ClientStreamingMethod<Req, Resp>,
      io.grpc.stub.ServerCalls.BidiStreamingMethod<Req, Resp> {
    private final MasterNodeServiceImplBase serviceImpl;
    private final int methodId;

    MethodHandlers(MasterNodeServiceImplBase serviceImpl, int methodId) {
      this.serviceImpl = serviceImpl;
      this.methodId = methodId;
    }

    @java.lang.Override
    @java.lang.SuppressWarnings("unchecked")
    public void invoke(Req request, io.grpc.stub.StreamObserver<Resp> responseObserver) {
      switch (methodId) {
        case METHODID_UPLOAD_TASK:
          serviceImpl.uploadTask((nodepool.UploadTaskRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.UploadTaskResponse>) responseObserver);
          break;
        case METHODID_POLL_TASK_STATUS:
          serviceImpl.pollTaskStatus((nodepool.PollTaskStatusRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.PollTaskStatusResponse>) responseObserver);
          break;
        case METHODID_STORE_OUTPUT:
          serviceImpl.storeOutput((nodepool.StoreOutputRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.StatusResponse>) responseObserver);
          break;
        case METHODID_STORE_RESULT:
          serviceImpl.storeResult((nodepool.StoreResultRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.StatusResponse>) responseObserver);
          break;
        case METHODID_GET_TASK_RESULT:
          serviceImpl.getTaskResult((nodepool.GetTaskResultRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.GetTaskResultResponse>) responseObserver);
          break;
        case METHODID_TASK_COMPLETED:
          serviceImpl.taskCompleted((nodepool.TaskCompletedRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.StatusResponse>) responseObserver);
          break;
        case METHODID_STORE_LOGS:
          serviceImpl.storeLogs((nodepool.StoreLogsRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.StatusResponse>) responseObserver);
          break;
        case METHODID_GET_TASK_LOGS:
          serviceImpl.getTaskLogs((nodepool.GetTaskLogsRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.GetTaskLogsResponse>) responseObserver);
          break;
        case METHODID_GET_ALL_TASKS:
          serviceImpl.getAllTasks((nodepool.GetAllTasksRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.GetAllTasksResponse>) responseObserver);
          break;
        case METHODID_STOP_TASK:
          serviceImpl.stopTask((nodepool.StopTaskRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.StopTaskResponse>) responseObserver);
          break;
        case METHODID_RETURN_TASK_RESULT:
          serviceImpl.returnTaskResult((nodepool.ReturnTaskResultRequest) request,
              (io.grpc.stub.StreamObserver<nodepool.ReturnTaskResultResponse>) responseObserver);
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

  private static abstract class MasterNodeServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoFileDescriptorSupplier, io.grpc.protobuf.ProtoServiceDescriptorSupplier {
    MasterNodeServiceBaseDescriptorSupplier() {}

    @java.lang.Override
    public com.google.protobuf.Descriptors.FileDescriptor getFileDescriptor() {
      return nodepool.NodepoolOuterClass.getDescriptor();
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.ServiceDescriptor getServiceDescriptor() {
      return getFileDescriptor().findServiceByName("MasterNodeService");
    }
  }

  private static final class MasterNodeServiceFileDescriptorSupplier
      extends MasterNodeServiceBaseDescriptorSupplier {
    MasterNodeServiceFileDescriptorSupplier() {}
  }

  private static final class MasterNodeServiceMethodDescriptorSupplier
      extends MasterNodeServiceBaseDescriptorSupplier
      implements io.grpc.protobuf.ProtoMethodDescriptorSupplier {
    private final String methodName;

    MasterNodeServiceMethodDescriptorSupplier(String methodName) {
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
      synchronized (MasterNodeServiceGrpc.class) {
        result = serviceDescriptor;
        if (result == null) {
          serviceDescriptor = result = io.grpc.ServiceDescriptor.newBuilder(SERVICE_NAME)
              .setSchemaDescriptor(new MasterNodeServiceFileDescriptorSupplier())
              .addMethod(getUploadTaskMethod())
              .addMethod(getPollTaskStatusMethod())
              .addMethod(getStoreOutputMethod())
              .addMethod(getStoreResultMethod())
              .addMethod(getGetTaskResultMethod())
              .addMethod(getTaskCompletedMethod())
              .addMethod(getStoreLogsMethod())
              .addMethod(getGetTaskLogsMethod())
              .addMethod(getGetAllTasksMethod())
              .addMethod(getStopTaskMethod())
              .addMethod(getReturnTaskResultMethod())
              .build();
        }
      }
    }
    return result;
  }
}
