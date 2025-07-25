// Generated by the protocol buffer compiler.  DO NOT EDIT!
// NO CHECKED-IN PROTOBUF GENCODE
// source: nodepool.proto
// Protobuf Java Version: 4.31.1

package nodepool;

/**
 * Protobuf type {@code nodepool.UploadTaskRequest}
 */
@com.google.protobuf.Generated
public final class UploadTaskRequest extends
    com.google.protobuf.GeneratedMessage implements
    // @@protoc_insertion_point(message_implements:nodepool.UploadTaskRequest)
    UploadTaskRequestOrBuilder {
private static final long serialVersionUID = 0L;
  static {
    com.google.protobuf.RuntimeVersion.validateProtobufGencodeVersion(
      com.google.protobuf.RuntimeVersion.RuntimeDomain.PUBLIC,
      /* major= */ 4,
      /* minor= */ 31,
      /* patch= */ 1,
      /* suffix= */ "",
      UploadTaskRequest.class.getName());
  }
  // Use UploadTaskRequest.newBuilder() to construct.
  private UploadTaskRequest(com.google.protobuf.GeneratedMessage.Builder<?> builder) {
    super(builder);
  }
  private UploadTaskRequest() {
    taskId_ = "";
    taskZip_ = com.google.protobuf.ByteString.EMPTY;
    location_ = "";
    gpuName_ = "";
    userId_ = "";
  }

  public static final com.google.protobuf.Descriptors.Descriptor
      getDescriptor() {
    return nodepool.NodepoolOuterClass.internal_static_nodepool_UploadTaskRequest_descriptor;
  }

  @java.lang.Override
  protected com.google.protobuf.GeneratedMessage.FieldAccessorTable
      internalGetFieldAccessorTable() {
    return nodepool.NodepoolOuterClass.internal_static_nodepool_UploadTaskRequest_fieldAccessorTable
        .ensureFieldAccessorsInitialized(
            nodepool.UploadTaskRequest.class, nodepool.UploadTaskRequest.Builder.class);
  }

  public static final int TASK_ID_FIELD_NUMBER = 1;
  @SuppressWarnings("serial")
  private volatile java.lang.Object taskId_ = "";
  /**
   * <code>string task_id = 1;</code>
   * @return The taskId.
   */
  @java.lang.Override
  public java.lang.String getTaskId() {
    java.lang.Object ref = taskId_;
    if (ref instanceof java.lang.String) {
      return (java.lang.String) ref;
    } else {
      com.google.protobuf.ByteString bs = 
          (com.google.protobuf.ByteString) ref;
      java.lang.String s = bs.toStringUtf8();
      taskId_ = s;
      return s;
    }
  }
  /**
   * <code>string task_id = 1;</code>
   * @return The bytes for taskId.
   */
  @java.lang.Override
  public com.google.protobuf.ByteString
      getTaskIdBytes() {
    java.lang.Object ref = taskId_;
    if (ref instanceof java.lang.String) {
      com.google.protobuf.ByteString b = 
          com.google.protobuf.ByteString.copyFromUtf8(
              (java.lang.String) ref);
      taskId_ = b;
      return b;
    } else {
      return (com.google.protobuf.ByteString) ref;
    }
  }

  public static final int TASK_ZIP_FIELD_NUMBER = 2;
  private com.google.protobuf.ByteString taskZip_ = com.google.protobuf.ByteString.EMPTY;
  /**
   * <code>bytes task_zip = 2;</code>
   * @return The taskZip.
   */
  @java.lang.Override
  public com.google.protobuf.ByteString getTaskZip() {
    return taskZip_;
  }

  public static final int MEMORY_GB_FIELD_NUMBER = 3;
  private int memoryGb_ = 0;
  /**
   * <code>int32 memory_gb = 3;</code>
   * @return The memoryGb.
   */
  @java.lang.Override
  public int getMemoryGb() {
    return memoryGb_;
  }

  public static final int CPU_SCORE_FIELD_NUMBER = 4;
  private int cpuScore_ = 0;
  /**
   * <code>int32 cpu_score = 4;</code>
   * @return The cpuScore.
   */
  @java.lang.Override
  public int getCpuScore() {
    return cpuScore_;
  }

  public static final int GPU_SCORE_FIELD_NUMBER = 5;
  private int gpuScore_ = 0;
  /**
   * <code>int32 gpu_score = 5;</code>
   * @return The gpuScore.
   */
  @java.lang.Override
  public int getGpuScore() {
    return gpuScore_;
  }

  public static final int GPU_MEMORY_GB_FIELD_NUMBER = 6;
  private int gpuMemoryGb_ = 0;
  /**
   * <code>int32 gpu_memory_gb = 6;</code>
   * @return The gpuMemoryGb.
   */
  @java.lang.Override
  public int getGpuMemoryGb() {
    return gpuMemoryGb_;
  }

  public static final int LOCATION_FIELD_NUMBER = 7;
  @SuppressWarnings("serial")
  private volatile java.lang.Object location_ = "";
  /**
   * <code>string location = 7;</code>
   * @return The location.
   */
  @java.lang.Override
  public java.lang.String getLocation() {
    java.lang.Object ref = location_;
    if (ref instanceof java.lang.String) {
      return (java.lang.String) ref;
    } else {
      com.google.protobuf.ByteString bs = 
          (com.google.protobuf.ByteString) ref;
      java.lang.String s = bs.toStringUtf8();
      location_ = s;
      return s;
    }
  }
  /**
   * <code>string location = 7;</code>
   * @return The bytes for location.
   */
  @java.lang.Override
  public com.google.protobuf.ByteString
      getLocationBytes() {
    java.lang.Object ref = location_;
    if (ref instanceof java.lang.String) {
      com.google.protobuf.ByteString b = 
          com.google.protobuf.ByteString.copyFromUtf8(
              (java.lang.String) ref);
      location_ = b;
      return b;
    } else {
      return (com.google.protobuf.ByteString) ref;
    }
  }

  public static final int GPU_NAME_FIELD_NUMBER = 8;
  @SuppressWarnings("serial")
  private volatile java.lang.Object gpuName_ = "";
  /**
   * <code>string gpu_name = 8;</code>
   * @return The gpuName.
   */
  @java.lang.Override
  public java.lang.String getGpuName() {
    java.lang.Object ref = gpuName_;
    if (ref instanceof java.lang.String) {
      return (java.lang.String) ref;
    } else {
      com.google.protobuf.ByteString bs = 
          (com.google.protobuf.ByteString) ref;
      java.lang.String s = bs.toStringUtf8();
      gpuName_ = s;
      return s;
    }
  }
  /**
   * <code>string gpu_name = 8;</code>
   * @return The bytes for gpuName.
   */
  @java.lang.Override
  public com.google.protobuf.ByteString
      getGpuNameBytes() {
    java.lang.Object ref = gpuName_;
    if (ref instanceof java.lang.String) {
      com.google.protobuf.ByteString b = 
          com.google.protobuf.ByteString.copyFromUtf8(
              (java.lang.String) ref);
      gpuName_ = b;
      return b;
    } else {
      return (com.google.protobuf.ByteString) ref;
    }
  }

  public static final int USER_ID_FIELD_NUMBER = 9;
  @SuppressWarnings("serial")
  private volatile java.lang.Object userId_ = "";
  /**
   * <pre>
   * 添加用戶ID字段
   * </pre>
   *
   * <code>string user_id = 9;</code>
   * @return The userId.
   */
  @java.lang.Override
  public java.lang.String getUserId() {
    java.lang.Object ref = userId_;
    if (ref instanceof java.lang.String) {
      return (java.lang.String) ref;
    } else {
      com.google.protobuf.ByteString bs = 
          (com.google.protobuf.ByteString) ref;
      java.lang.String s = bs.toStringUtf8();
      userId_ = s;
      return s;
    }
  }
  /**
   * <pre>
   * 添加用戶ID字段
   * </pre>
   *
   * <code>string user_id = 9;</code>
   * @return The bytes for userId.
   */
  @java.lang.Override
  public com.google.protobuf.ByteString
      getUserIdBytes() {
    java.lang.Object ref = userId_;
    if (ref instanceof java.lang.String) {
      com.google.protobuf.ByteString b = 
          com.google.protobuf.ByteString.copyFromUtf8(
              (java.lang.String) ref);
      userId_ = b;
      return b;
    } else {
      return (com.google.protobuf.ByteString) ref;
    }
  }

  private byte memoizedIsInitialized = -1;
  @java.lang.Override
  public final boolean isInitialized() {
    byte isInitialized = memoizedIsInitialized;
    if (isInitialized == 1) return true;
    if (isInitialized == 0) return false;

    memoizedIsInitialized = 1;
    return true;
  }

  @java.lang.Override
  public void writeTo(com.google.protobuf.CodedOutputStream output)
                      throws java.io.IOException {
    if (!com.google.protobuf.GeneratedMessage.isStringEmpty(taskId_)) {
      com.google.protobuf.GeneratedMessage.writeString(output, 1, taskId_);
    }
    if (!taskZip_.isEmpty()) {
      output.writeBytes(2, taskZip_);
    }
    if (memoryGb_ != 0) {
      output.writeInt32(3, memoryGb_);
    }
    if (cpuScore_ != 0) {
      output.writeInt32(4, cpuScore_);
    }
    if (gpuScore_ != 0) {
      output.writeInt32(5, gpuScore_);
    }
    if (gpuMemoryGb_ != 0) {
      output.writeInt32(6, gpuMemoryGb_);
    }
    if (!com.google.protobuf.GeneratedMessage.isStringEmpty(location_)) {
      com.google.protobuf.GeneratedMessage.writeString(output, 7, location_);
    }
    if (!com.google.protobuf.GeneratedMessage.isStringEmpty(gpuName_)) {
      com.google.protobuf.GeneratedMessage.writeString(output, 8, gpuName_);
    }
    if (!com.google.protobuf.GeneratedMessage.isStringEmpty(userId_)) {
      com.google.protobuf.GeneratedMessage.writeString(output, 9, userId_);
    }
    getUnknownFields().writeTo(output);
  }

  @java.lang.Override
  public int getSerializedSize() {
    int size = memoizedSize;
    if (size != -1) return size;

    size = 0;
    if (!com.google.protobuf.GeneratedMessage.isStringEmpty(taskId_)) {
      size += com.google.protobuf.GeneratedMessage.computeStringSize(1, taskId_);
    }
    if (!taskZip_.isEmpty()) {
      size += com.google.protobuf.CodedOutputStream
        .computeBytesSize(2, taskZip_);
    }
    if (memoryGb_ != 0) {
      size += com.google.protobuf.CodedOutputStream
        .computeInt32Size(3, memoryGb_);
    }
    if (cpuScore_ != 0) {
      size += com.google.protobuf.CodedOutputStream
        .computeInt32Size(4, cpuScore_);
    }
    if (gpuScore_ != 0) {
      size += com.google.protobuf.CodedOutputStream
        .computeInt32Size(5, gpuScore_);
    }
    if (gpuMemoryGb_ != 0) {
      size += com.google.protobuf.CodedOutputStream
        .computeInt32Size(6, gpuMemoryGb_);
    }
    if (!com.google.protobuf.GeneratedMessage.isStringEmpty(location_)) {
      size += com.google.protobuf.GeneratedMessage.computeStringSize(7, location_);
    }
    if (!com.google.protobuf.GeneratedMessage.isStringEmpty(gpuName_)) {
      size += com.google.protobuf.GeneratedMessage.computeStringSize(8, gpuName_);
    }
    if (!com.google.protobuf.GeneratedMessage.isStringEmpty(userId_)) {
      size += com.google.protobuf.GeneratedMessage.computeStringSize(9, userId_);
    }
    size += getUnknownFields().getSerializedSize();
    memoizedSize = size;
    return size;
  }

  @java.lang.Override
  public boolean equals(final java.lang.Object obj) {
    if (obj == this) {
     return true;
    }
    if (!(obj instanceof nodepool.UploadTaskRequest)) {
      return super.equals(obj);
    }
    nodepool.UploadTaskRequest other = (nodepool.UploadTaskRequest) obj;

    if (!getTaskId()
        .equals(other.getTaskId())) return false;
    if (!getTaskZip()
        .equals(other.getTaskZip())) return false;
    if (getMemoryGb()
        != other.getMemoryGb()) return false;
    if (getCpuScore()
        != other.getCpuScore()) return false;
    if (getGpuScore()
        != other.getGpuScore()) return false;
    if (getGpuMemoryGb()
        != other.getGpuMemoryGb()) return false;
    if (!getLocation()
        .equals(other.getLocation())) return false;
    if (!getGpuName()
        .equals(other.getGpuName())) return false;
    if (!getUserId()
        .equals(other.getUserId())) return false;
    if (!getUnknownFields().equals(other.getUnknownFields())) return false;
    return true;
  }

  @java.lang.Override
  public int hashCode() {
    if (memoizedHashCode != 0) {
      return memoizedHashCode;
    }
    int hash = 41;
    hash = (19 * hash) + getDescriptor().hashCode();
    hash = (37 * hash) + TASK_ID_FIELD_NUMBER;
    hash = (53 * hash) + getTaskId().hashCode();
    hash = (37 * hash) + TASK_ZIP_FIELD_NUMBER;
    hash = (53 * hash) + getTaskZip().hashCode();
    hash = (37 * hash) + MEMORY_GB_FIELD_NUMBER;
    hash = (53 * hash) + getMemoryGb();
    hash = (37 * hash) + CPU_SCORE_FIELD_NUMBER;
    hash = (53 * hash) + getCpuScore();
    hash = (37 * hash) + GPU_SCORE_FIELD_NUMBER;
    hash = (53 * hash) + getGpuScore();
    hash = (37 * hash) + GPU_MEMORY_GB_FIELD_NUMBER;
    hash = (53 * hash) + getGpuMemoryGb();
    hash = (37 * hash) + LOCATION_FIELD_NUMBER;
    hash = (53 * hash) + getLocation().hashCode();
    hash = (37 * hash) + GPU_NAME_FIELD_NUMBER;
    hash = (53 * hash) + getGpuName().hashCode();
    hash = (37 * hash) + USER_ID_FIELD_NUMBER;
    hash = (53 * hash) + getUserId().hashCode();
    hash = (29 * hash) + getUnknownFields().hashCode();
    memoizedHashCode = hash;
    return hash;
  }

  public static nodepool.UploadTaskRequest parseFrom(
      java.nio.ByteBuffer data)
      throws com.google.protobuf.InvalidProtocolBufferException {
    return PARSER.parseFrom(data);
  }
  public static nodepool.UploadTaskRequest parseFrom(
      java.nio.ByteBuffer data,
      com.google.protobuf.ExtensionRegistryLite extensionRegistry)
      throws com.google.protobuf.InvalidProtocolBufferException {
    return PARSER.parseFrom(data, extensionRegistry);
  }
  public static nodepool.UploadTaskRequest parseFrom(
      com.google.protobuf.ByteString data)
      throws com.google.protobuf.InvalidProtocolBufferException {
    return PARSER.parseFrom(data);
  }
  public static nodepool.UploadTaskRequest parseFrom(
      com.google.protobuf.ByteString data,
      com.google.protobuf.ExtensionRegistryLite extensionRegistry)
      throws com.google.protobuf.InvalidProtocolBufferException {
    return PARSER.parseFrom(data, extensionRegistry);
  }
  public static nodepool.UploadTaskRequest parseFrom(byte[] data)
      throws com.google.protobuf.InvalidProtocolBufferException {
    return PARSER.parseFrom(data);
  }
  public static nodepool.UploadTaskRequest parseFrom(
      byte[] data,
      com.google.protobuf.ExtensionRegistryLite extensionRegistry)
      throws com.google.protobuf.InvalidProtocolBufferException {
    return PARSER.parseFrom(data, extensionRegistry);
  }
  public static nodepool.UploadTaskRequest parseFrom(java.io.InputStream input)
      throws java.io.IOException {
    return com.google.protobuf.GeneratedMessage
        .parseWithIOException(PARSER, input);
  }
  public static nodepool.UploadTaskRequest parseFrom(
      java.io.InputStream input,
      com.google.protobuf.ExtensionRegistryLite extensionRegistry)
      throws java.io.IOException {
    return com.google.protobuf.GeneratedMessage
        .parseWithIOException(PARSER, input, extensionRegistry);
  }

  public static nodepool.UploadTaskRequest parseDelimitedFrom(java.io.InputStream input)
      throws java.io.IOException {
    return com.google.protobuf.GeneratedMessage
        .parseDelimitedWithIOException(PARSER, input);
  }

  public static nodepool.UploadTaskRequest parseDelimitedFrom(
      java.io.InputStream input,
      com.google.protobuf.ExtensionRegistryLite extensionRegistry)
      throws java.io.IOException {
    return com.google.protobuf.GeneratedMessage
        .parseDelimitedWithIOException(PARSER, input, extensionRegistry);
  }
  public static nodepool.UploadTaskRequest parseFrom(
      com.google.protobuf.CodedInputStream input)
      throws java.io.IOException {
    return com.google.protobuf.GeneratedMessage
        .parseWithIOException(PARSER, input);
  }
  public static nodepool.UploadTaskRequest parseFrom(
      com.google.protobuf.CodedInputStream input,
      com.google.protobuf.ExtensionRegistryLite extensionRegistry)
      throws java.io.IOException {
    return com.google.protobuf.GeneratedMessage
        .parseWithIOException(PARSER, input, extensionRegistry);
  }

  @java.lang.Override
  public Builder newBuilderForType() { return newBuilder(); }
  public static Builder newBuilder() {
    return DEFAULT_INSTANCE.toBuilder();
  }
  public static Builder newBuilder(nodepool.UploadTaskRequest prototype) {
    return DEFAULT_INSTANCE.toBuilder().mergeFrom(prototype);
  }
  @java.lang.Override
  public Builder toBuilder() {
    return this == DEFAULT_INSTANCE
        ? new Builder() : new Builder().mergeFrom(this);
  }

  @java.lang.Override
  protected Builder newBuilderForType(
      com.google.protobuf.GeneratedMessage.BuilderParent parent) {
    Builder builder = new Builder(parent);
    return builder;
  }
  /**
   * Protobuf type {@code nodepool.UploadTaskRequest}
   */
  public static final class Builder extends
      com.google.protobuf.GeneratedMessage.Builder<Builder> implements
      // @@protoc_insertion_point(builder_implements:nodepool.UploadTaskRequest)
      nodepool.UploadTaskRequestOrBuilder {
    public static final com.google.protobuf.Descriptors.Descriptor
        getDescriptor() {
      return nodepool.NodepoolOuterClass.internal_static_nodepool_UploadTaskRequest_descriptor;
    }

    @java.lang.Override
    protected com.google.protobuf.GeneratedMessage.FieldAccessorTable
        internalGetFieldAccessorTable() {
      return nodepool.NodepoolOuterClass.internal_static_nodepool_UploadTaskRequest_fieldAccessorTable
          .ensureFieldAccessorsInitialized(
              nodepool.UploadTaskRequest.class, nodepool.UploadTaskRequest.Builder.class);
    }

    // Construct using nodepool.UploadTaskRequest.newBuilder()
    private Builder() {

    }

    private Builder(
        com.google.protobuf.GeneratedMessage.BuilderParent parent) {
      super(parent);

    }
    @java.lang.Override
    public Builder clear() {
      super.clear();
      bitField0_ = 0;
      taskId_ = "";
      taskZip_ = com.google.protobuf.ByteString.EMPTY;
      memoryGb_ = 0;
      cpuScore_ = 0;
      gpuScore_ = 0;
      gpuMemoryGb_ = 0;
      location_ = "";
      gpuName_ = "";
      userId_ = "";
      return this;
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.Descriptor
        getDescriptorForType() {
      return nodepool.NodepoolOuterClass.internal_static_nodepool_UploadTaskRequest_descriptor;
    }

    @java.lang.Override
    public nodepool.UploadTaskRequest getDefaultInstanceForType() {
      return nodepool.UploadTaskRequest.getDefaultInstance();
    }

    @java.lang.Override
    public nodepool.UploadTaskRequest build() {
      nodepool.UploadTaskRequest result = buildPartial();
      if (!result.isInitialized()) {
        throw newUninitializedMessageException(result);
      }
      return result;
    }

    @java.lang.Override
    public nodepool.UploadTaskRequest buildPartial() {
      nodepool.UploadTaskRequest result = new nodepool.UploadTaskRequest(this);
      if (bitField0_ != 0) { buildPartial0(result); }
      onBuilt();
      return result;
    }

    private void buildPartial0(nodepool.UploadTaskRequest result) {
      int from_bitField0_ = bitField0_;
      if (((from_bitField0_ & 0x00000001) != 0)) {
        result.taskId_ = taskId_;
      }
      if (((from_bitField0_ & 0x00000002) != 0)) {
        result.taskZip_ = taskZip_;
      }
      if (((from_bitField0_ & 0x00000004) != 0)) {
        result.memoryGb_ = memoryGb_;
      }
      if (((from_bitField0_ & 0x00000008) != 0)) {
        result.cpuScore_ = cpuScore_;
      }
      if (((from_bitField0_ & 0x00000010) != 0)) {
        result.gpuScore_ = gpuScore_;
      }
      if (((from_bitField0_ & 0x00000020) != 0)) {
        result.gpuMemoryGb_ = gpuMemoryGb_;
      }
      if (((from_bitField0_ & 0x00000040) != 0)) {
        result.location_ = location_;
      }
      if (((from_bitField0_ & 0x00000080) != 0)) {
        result.gpuName_ = gpuName_;
      }
      if (((from_bitField0_ & 0x00000100) != 0)) {
        result.userId_ = userId_;
      }
    }

    @java.lang.Override
    public Builder mergeFrom(com.google.protobuf.Message other) {
      if (other instanceof nodepool.UploadTaskRequest) {
        return mergeFrom((nodepool.UploadTaskRequest)other);
      } else {
        super.mergeFrom(other);
        return this;
      }
    }

    public Builder mergeFrom(nodepool.UploadTaskRequest other) {
      if (other == nodepool.UploadTaskRequest.getDefaultInstance()) return this;
      if (!other.getTaskId().isEmpty()) {
        taskId_ = other.taskId_;
        bitField0_ |= 0x00000001;
        onChanged();
      }
      if (!other.getTaskZip().isEmpty()) {
        setTaskZip(other.getTaskZip());
      }
      if (other.getMemoryGb() != 0) {
        setMemoryGb(other.getMemoryGb());
      }
      if (other.getCpuScore() != 0) {
        setCpuScore(other.getCpuScore());
      }
      if (other.getGpuScore() != 0) {
        setGpuScore(other.getGpuScore());
      }
      if (other.getGpuMemoryGb() != 0) {
        setGpuMemoryGb(other.getGpuMemoryGb());
      }
      if (!other.getLocation().isEmpty()) {
        location_ = other.location_;
        bitField0_ |= 0x00000040;
        onChanged();
      }
      if (!other.getGpuName().isEmpty()) {
        gpuName_ = other.gpuName_;
        bitField0_ |= 0x00000080;
        onChanged();
      }
      if (!other.getUserId().isEmpty()) {
        userId_ = other.userId_;
        bitField0_ |= 0x00000100;
        onChanged();
      }
      this.mergeUnknownFields(other.getUnknownFields());
      onChanged();
      return this;
    }

    @java.lang.Override
    public final boolean isInitialized() {
      return true;
    }

    @java.lang.Override
    public Builder mergeFrom(
        com.google.protobuf.CodedInputStream input,
        com.google.protobuf.ExtensionRegistryLite extensionRegistry)
        throws java.io.IOException {
      if (extensionRegistry == null) {
        throw new java.lang.NullPointerException();
      }
      try {
        boolean done = false;
        while (!done) {
          int tag = input.readTag();
          switch (tag) {
            case 0:
              done = true;
              break;
            case 10: {
              taskId_ = input.readStringRequireUtf8();
              bitField0_ |= 0x00000001;
              break;
            } // case 10
            case 18: {
              taskZip_ = input.readBytes();
              bitField0_ |= 0x00000002;
              break;
            } // case 18
            case 24: {
              memoryGb_ = input.readInt32();
              bitField0_ |= 0x00000004;
              break;
            } // case 24
            case 32: {
              cpuScore_ = input.readInt32();
              bitField0_ |= 0x00000008;
              break;
            } // case 32
            case 40: {
              gpuScore_ = input.readInt32();
              bitField0_ |= 0x00000010;
              break;
            } // case 40
            case 48: {
              gpuMemoryGb_ = input.readInt32();
              bitField0_ |= 0x00000020;
              break;
            } // case 48
            case 58: {
              location_ = input.readStringRequireUtf8();
              bitField0_ |= 0x00000040;
              break;
            } // case 58
            case 66: {
              gpuName_ = input.readStringRequireUtf8();
              bitField0_ |= 0x00000080;
              break;
            } // case 66
            case 74: {
              userId_ = input.readStringRequireUtf8();
              bitField0_ |= 0x00000100;
              break;
            } // case 74
            default: {
              if (!super.parseUnknownField(input, extensionRegistry, tag)) {
                done = true; // was an endgroup tag
              }
              break;
            } // default:
          } // switch (tag)
        } // while (!done)
      } catch (com.google.protobuf.InvalidProtocolBufferException e) {
        throw e.unwrapIOException();
      } finally {
        onChanged();
      } // finally
      return this;
    }
    private int bitField0_;

    private java.lang.Object taskId_ = "";
    /**
     * <code>string task_id = 1;</code>
     * @return The taskId.
     */
    public java.lang.String getTaskId() {
      java.lang.Object ref = taskId_;
      if (!(ref instanceof java.lang.String)) {
        com.google.protobuf.ByteString bs =
            (com.google.protobuf.ByteString) ref;
        java.lang.String s = bs.toStringUtf8();
        taskId_ = s;
        return s;
      } else {
        return (java.lang.String) ref;
      }
    }
    /**
     * <code>string task_id = 1;</code>
     * @return The bytes for taskId.
     */
    public com.google.protobuf.ByteString
        getTaskIdBytes() {
      java.lang.Object ref = taskId_;
      if (ref instanceof String) {
        com.google.protobuf.ByteString b = 
            com.google.protobuf.ByteString.copyFromUtf8(
                (java.lang.String) ref);
        taskId_ = b;
        return b;
      } else {
        return (com.google.protobuf.ByteString) ref;
      }
    }
    /**
     * <code>string task_id = 1;</code>
     * @param value The taskId to set.
     * @return This builder for chaining.
     */
    public Builder setTaskId(
        java.lang.String value) {
      if (value == null) { throw new NullPointerException(); }
      taskId_ = value;
      bitField0_ |= 0x00000001;
      onChanged();
      return this;
    }
    /**
     * <code>string task_id = 1;</code>
     * @return This builder for chaining.
     */
    public Builder clearTaskId() {
      taskId_ = getDefaultInstance().getTaskId();
      bitField0_ = (bitField0_ & ~0x00000001);
      onChanged();
      return this;
    }
    /**
     * <code>string task_id = 1;</code>
     * @param value The bytes for taskId to set.
     * @return This builder for chaining.
     */
    public Builder setTaskIdBytes(
        com.google.protobuf.ByteString value) {
      if (value == null) { throw new NullPointerException(); }
      checkByteStringIsUtf8(value);
      taskId_ = value;
      bitField0_ |= 0x00000001;
      onChanged();
      return this;
    }

    private com.google.protobuf.ByteString taskZip_ = com.google.protobuf.ByteString.EMPTY;
    /**
     * <code>bytes task_zip = 2;</code>
     * @return The taskZip.
     */
    @java.lang.Override
    public com.google.protobuf.ByteString getTaskZip() {
      return taskZip_;
    }
    /**
     * <code>bytes task_zip = 2;</code>
     * @param value The taskZip to set.
     * @return This builder for chaining.
     */
    public Builder setTaskZip(com.google.protobuf.ByteString value) {
      if (value == null) { throw new NullPointerException(); }
      taskZip_ = value;
      bitField0_ |= 0x00000002;
      onChanged();
      return this;
    }
    /**
     * <code>bytes task_zip = 2;</code>
     * @return This builder for chaining.
     */
    public Builder clearTaskZip() {
      bitField0_ = (bitField0_ & ~0x00000002);
      taskZip_ = getDefaultInstance().getTaskZip();
      onChanged();
      return this;
    }

    private int memoryGb_ ;
    /**
     * <code>int32 memory_gb = 3;</code>
     * @return The memoryGb.
     */
    @java.lang.Override
    public int getMemoryGb() {
      return memoryGb_;
    }
    /**
     * <code>int32 memory_gb = 3;</code>
     * @param value The memoryGb to set.
     * @return This builder for chaining.
     */
    public Builder setMemoryGb(int value) {

      memoryGb_ = value;
      bitField0_ |= 0x00000004;
      onChanged();
      return this;
    }
    /**
     * <code>int32 memory_gb = 3;</code>
     * @return This builder for chaining.
     */
    public Builder clearMemoryGb() {
      bitField0_ = (bitField0_ & ~0x00000004);
      memoryGb_ = 0;
      onChanged();
      return this;
    }

    private int cpuScore_ ;
    /**
     * <code>int32 cpu_score = 4;</code>
     * @return The cpuScore.
     */
    @java.lang.Override
    public int getCpuScore() {
      return cpuScore_;
    }
    /**
     * <code>int32 cpu_score = 4;</code>
     * @param value The cpuScore to set.
     * @return This builder for chaining.
     */
    public Builder setCpuScore(int value) {

      cpuScore_ = value;
      bitField0_ |= 0x00000008;
      onChanged();
      return this;
    }
    /**
     * <code>int32 cpu_score = 4;</code>
     * @return This builder for chaining.
     */
    public Builder clearCpuScore() {
      bitField0_ = (bitField0_ & ~0x00000008);
      cpuScore_ = 0;
      onChanged();
      return this;
    }

    private int gpuScore_ ;
    /**
     * <code>int32 gpu_score = 5;</code>
     * @return The gpuScore.
     */
    @java.lang.Override
    public int getGpuScore() {
      return gpuScore_;
    }
    /**
     * <code>int32 gpu_score = 5;</code>
     * @param value The gpuScore to set.
     * @return This builder for chaining.
     */
    public Builder setGpuScore(int value) {

      gpuScore_ = value;
      bitField0_ |= 0x00000010;
      onChanged();
      return this;
    }
    /**
     * <code>int32 gpu_score = 5;</code>
     * @return This builder for chaining.
     */
    public Builder clearGpuScore() {
      bitField0_ = (bitField0_ & ~0x00000010);
      gpuScore_ = 0;
      onChanged();
      return this;
    }

    private int gpuMemoryGb_ ;
    /**
     * <code>int32 gpu_memory_gb = 6;</code>
     * @return The gpuMemoryGb.
     */
    @java.lang.Override
    public int getGpuMemoryGb() {
      return gpuMemoryGb_;
    }
    /**
     * <code>int32 gpu_memory_gb = 6;</code>
     * @param value The gpuMemoryGb to set.
     * @return This builder for chaining.
     */
    public Builder setGpuMemoryGb(int value) {

      gpuMemoryGb_ = value;
      bitField0_ |= 0x00000020;
      onChanged();
      return this;
    }
    /**
     * <code>int32 gpu_memory_gb = 6;</code>
     * @return This builder for chaining.
     */
    public Builder clearGpuMemoryGb() {
      bitField0_ = (bitField0_ & ~0x00000020);
      gpuMemoryGb_ = 0;
      onChanged();
      return this;
    }

    private java.lang.Object location_ = "";
    /**
     * <code>string location = 7;</code>
     * @return The location.
     */
    public java.lang.String getLocation() {
      java.lang.Object ref = location_;
      if (!(ref instanceof java.lang.String)) {
        com.google.protobuf.ByteString bs =
            (com.google.protobuf.ByteString) ref;
        java.lang.String s = bs.toStringUtf8();
        location_ = s;
        return s;
      } else {
        return (java.lang.String) ref;
      }
    }
    /**
     * <code>string location = 7;</code>
     * @return The bytes for location.
     */
    public com.google.protobuf.ByteString
        getLocationBytes() {
      java.lang.Object ref = location_;
      if (ref instanceof String) {
        com.google.protobuf.ByteString b = 
            com.google.protobuf.ByteString.copyFromUtf8(
                (java.lang.String) ref);
        location_ = b;
        return b;
      } else {
        return (com.google.protobuf.ByteString) ref;
      }
    }
    /**
     * <code>string location = 7;</code>
     * @param value The location to set.
     * @return This builder for chaining.
     */
    public Builder setLocation(
        java.lang.String value) {
      if (value == null) { throw new NullPointerException(); }
      location_ = value;
      bitField0_ |= 0x00000040;
      onChanged();
      return this;
    }
    /**
     * <code>string location = 7;</code>
     * @return This builder for chaining.
     */
    public Builder clearLocation() {
      location_ = getDefaultInstance().getLocation();
      bitField0_ = (bitField0_ & ~0x00000040);
      onChanged();
      return this;
    }
    /**
     * <code>string location = 7;</code>
     * @param value The bytes for location to set.
     * @return This builder for chaining.
     */
    public Builder setLocationBytes(
        com.google.protobuf.ByteString value) {
      if (value == null) { throw new NullPointerException(); }
      checkByteStringIsUtf8(value);
      location_ = value;
      bitField0_ |= 0x00000040;
      onChanged();
      return this;
    }

    private java.lang.Object gpuName_ = "";
    /**
     * <code>string gpu_name = 8;</code>
     * @return The gpuName.
     */
    public java.lang.String getGpuName() {
      java.lang.Object ref = gpuName_;
      if (!(ref instanceof java.lang.String)) {
        com.google.protobuf.ByteString bs =
            (com.google.protobuf.ByteString) ref;
        java.lang.String s = bs.toStringUtf8();
        gpuName_ = s;
        return s;
      } else {
        return (java.lang.String) ref;
      }
    }
    /**
     * <code>string gpu_name = 8;</code>
     * @return The bytes for gpuName.
     */
    public com.google.protobuf.ByteString
        getGpuNameBytes() {
      java.lang.Object ref = gpuName_;
      if (ref instanceof String) {
        com.google.protobuf.ByteString b = 
            com.google.protobuf.ByteString.copyFromUtf8(
                (java.lang.String) ref);
        gpuName_ = b;
        return b;
      } else {
        return (com.google.protobuf.ByteString) ref;
      }
    }
    /**
     * <code>string gpu_name = 8;</code>
     * @param value The gpuName to set.
     * @return This builder for chaining.
     */
    public Builder setGpuName(
        java.lang.String value) {
      if (value == null) { throw new NullPointerException(); }
      gpuName_ = value;
      bitField0_ |= 0x00000080;
      onChanged();
      return this;
    }
    /**
     * <code>string gpu_name = 8;</code>
     * @return This builder for chaining.
     */
    public Builder clearGpuName() {
      gpuName_ = getDefaultInstance().getGpuName();
      bitField0_ = (bitField0_ & ~0x00000080);
      onChanged();
      return this;
    }
    /**
     * <code>string gpu_name = 8;</code>
     * @param value The bytes for gpuName to set.
     * @return This builder for chaining.
     */
    public Builder setGpuNameBytes(
        com.google.protobuf.ByteString value) {
      if (value == null) { throw new NullPointerException(); }
      checkByteStringIsUtf8(value);
      gpuName_ = value;
      bitField0_ |= 0x00000080;
      onChanged();
      return this;
    }

    private java.lang.Object userId_ = "";
    /**
     * <pre>
     * 添加用戶ID字段
     * </pre>
     *
     * <code>string user_id = 9;</code>
     * @return The userId.
     */
    public java.lang.String getUserId() {
      java.lang.Object ref = userId_;
      if (!(ref instanceof java.lang.String)) {
        com.google.protobuf.ByteString bs =
            (com.google.protobuf.ByteString) ref;
        java.lang.String s = bs.toStringUtf8();
        userId_ = s;
        return s;
      } else {
        return (java.lang.String) ref;
      }
    }
    /**
     * <pre>
     * 添加用戶ID字段
     * </pre>
     *
     * <code>string user_id = 9;</code>
     * @return The bytes for userId.
     */
    public com.google.protobuf.ByteString
        getUserIdBytes() {
      java.lang.Object ref = userId_;
      if (ref instanceof String) {
        com.google.protobuf.ByteString b = 
            com.google.protobuf.ByteString.copyFromUtf8(
                (java.lang.String) ref);
        userId_ = b;
        return b;
      } else {
        return (com.google.protobuf.ByteString) ref;
      }
    }
    /**
     * <pre>
     * 添加用戶ID字段
     * </pre>
     *
     * <code>string user_id = 9;</code>
     * @param value The userId to set.
     * @return This builder for chaining.
     */
    public Builder setUserId(
        java.lang.String value) {
      if (value == null) { throw new NullPointerException(); }
      userId_ = value;
      bitField0_ |= 0x00000100;
      onChanged();
      return this;
    }
    /**
     * <pre>
     * 添加用戶ID字段
     * </pre>
     *
     * <code>string user_id = 9;</code>
     * @return This builder for chaining.
     */
    public Builder clearUserId() {
      userId_ = getDefaultInstance().getUserId();
      bitField0_ = (bitField0_ & ~0x00000100);
      onChanged();
      return this;
    }
    /**
     * <pre>
     * 添加用戶ID字段
     * </pre>
     *
     * <code>string user_id = 9;</code>
     * @param value The bytes for userId to set.
     * @return This builder for chaining.
     */
    public Builder setUserIdBytes(
        com.google.protobuf.ByteString value) {
      if (value == null) { throw new NullPointerException(); }
      checkByteStringIsUtf8(value);
      userId_ = value;
      bitField0_ |= 0x00000100;
      onChanged();
      return this;
    }

    // @@protoc_insertion_point(builder_scope:nodepool.UploadTaskRequest)
  }

  // @@protoc_insertion_point(class_scope:nodepool.UploadTaskRequest)
  private static final nodepool.UploadTaskRequest DEFAULT_INSTANCE;
  static {
    DEFAULT_INSTANCE = new nodepool.UploadTaskRequest();
  }

  public static nodepool.UploadTaskRequest getDefaultInstance() {
    return DEFAULT_INSTANCE;
  }

  private static final com.google.protobuf.Parser<UploadTaskRequest>
      PARSER = new com.google.protobuf.AbstractParser<UploadTaskRequest>() {
    @java.lang.Override
    public UploadTaskRequest parsePartialFrom(
        com.google.protobuf.CodedInputStream input,
        com.google.protobuf.ExtensionRegistryLite extensionRegistry)
        throws com.google.protobuf.InvalidProtocolBufferException {
      Builder builder = newBuilder();
      try {
        builder.mergeFrom(input, extensionRegistry);
      } catch (com.google.protobuf.InvalidProtocolBufferException e) {
        throw e.setUnfinishedMessage(builder.buildPartial());
      } catch (com.google.protobuf.UninitializedMessageException e) {
        throw e.asInvalidProtocolBufferException().setUnfinishedMessage(builder.buildPartial());
      } catch (java.io.IOException e) {
        throw new com.google.protobuf.InvalidProtocolBufferException(e)
            .setUnfinishedMessage(builder.buildPartial());
      }
      return builder.buildPartial();
    }
  };

  public static com.google.protobuf.Parser<UploadTaskRequest> parser() {
    return PARSER;
  }

  @java.lang.Override
  public com.google.protobuf.Parser<UploadTaskRequest> getParserForType() {
    return PARSER;
  }

  @java.lang.Override
  public nodepool.UploadTaskRequest getDefaultInstanceForType() {
    return DEFAULT_INSTANCE;
  }

}

