// Generated by the protocol buffer compiler.  DO NOT EDIT!
// NO CHECKED-IN PROTOBUF GENCODE
// source: nodepool.proto
// Protobuf Java Version: 4.31.1

package nodepool;

/**
 * Protobuf type {@code nodepool.StoreOutputRequest}
 */
@com.google.protobuf.Generated
public final class StoreOutputRequest extends
    com.google.protobuf.GeneratedMessage implements
    // @@protoc_insertion_point(message_implements:nodepool.StoreOutputRequest)
    StoreOutputRequestOrBuilder {
private static final long serialVersionUID = 0L;
  static {
    com.google.protobuf.RuntimeVersion.validateProtobufGencodeVersion(
      com.google.protobuf.RuntimeVersion.RuntimeDomain.PUBLIC,
      /* major= */ 4,
      /* minor= */ 31,
      /* patch= */ 1,
      /* suffix= */ "",
      StoreOutputRequest.class.getName());
  }
  // Use StoreOutputRequest.newBuilder() to construct.
  private StoreOutputRequest(com.google.protobuf.GeneratedMessage.Builder<?> builder) {
    super(builder);
  }
  private StoreOutputRequest() {
    taskId_ = "";
    output_ = "";
  }

  public static final com.google.protobuf.Descriptors.Descriptor
      getDescriptor() {
    return nodepool.NodepoolOuterClass.internal_static_nodepool_StoreOutputRequest_descriptor;
  }

  @java.lang.Override
  protected com.google.protobuf.GeneratedMessage.FieldAccessorTable
      internalGetFieldAccessorTable() {
    return nodepool.NodepoolOuterClass.internal_static_nodepool_StoreOutputRequest_fieldAccessorTable
        .ensureFieldAccessorsInitialized(
            nodepool.StoreOutputRequest.class, nodepool.StoreOutputRequest.Builder.class);
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

  public static final int OUTPUT_FIELD_NUMBER = 2;
  @SuppressWarnings("serial")
  private volatile java.lang.Object output_ = "";
  /**
   * <code>string output = 2;</code>
   * @return The output.
   */
  @java.lang.Override
  public java.lang.String getOutput() {
    java.lang.Object ref = output_;
    if (ref instanceof java.lang.String) {
      return (java.lang.String) ref;
    } else {
      com.google.protobuf.ByteString bs = 
          (com.google.protobuf.ByteString) ref;
      java.lang.String s = bs.toStringUtf8();
      output_ = s;
      return s;
    }
  }
  /**
   * <code>string output = 2;</code>
   * @return The bytes for output.
   */
  @java.lang.Override
  public com.google.protobuf.ByteString
      getOutputBytes() {
    java.lang.Object ref = output_;
    if (ref instanceof java.lang.String) {
      com.google.protobuf.ByteString b = 
          com.google.protobuf.ByteString.copyFromUtf8(
              (java.lang.String) ref);
      output_ = b;
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
    if (!com.google.protobuf.GeneratedMessage.isStringEmpty(output_)) {
      com.google.protobuf.GeneratedMessage.writeString(output, 2, output_);
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
    if (!com.google.protobuf.GeneratedMessage.isStringEmpty(output_)) {
      size += com.google.protobuf.GeneratedMessage.computeStringSize(2, output_);
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
    if (!(obj instanceof nodepool.StoreOutputRequest)) {
      return super.equals(obj);
    }
    nodepool.StoreOutputRequest other = (nodepool.StoreOutputRequest) obj;

    if (!getTaskId()
        .equals(other.getTaskId())) return false;
    if (!getOutput()
        .equals(other.getOutput())) return false;
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
    hash = (37 * hash) + OUTPUT_FIELD_NUMBER;
    hash = (53 * hash) + getOutput().hashCode();
    hash = (29 * hash) + getUnknownFields().hashCode();
    memoizedHashCode = hash;
    return hash;
  }

  public static nodepool.StoreOutputRequest parseFrom(
      java.nio.ByteBuffer data)
      throws com.google.protobuf.InvalidProtocolBufferException {
    return PARSER.parseFrom(data);
  }
  public static nodepool.StoreOutputRequest parseFrom(
      java.nio.ByteBuffer data,
      com.google.protobuf.ExtensionRegistryLite extensionRegistry)
      throws com.google.protobuf.InvalidProtocolBufferException {
    return PARSER.parseFrom(data, extensionRegistry);
  }
  public static nodepool.StoreOutputRequest parseFrom(
      com.google.protobuf.ByteString data)
      throws com.google.protobuf.InvalidProtocolBufferException {
    return PARSER.parseFrom(data);
  }
  public static nodepool.StoreOutputRequest parseFrom(
      com.google.protobuf.ByteString data,
      com.google.protobuf.ExtensionRegistryLite extensionRegistry)
      throws com.google.protobuf.InvalidProtocolBufferException {
    return PARSER.parseFrom(data, extensionRegistry);
  }
  public static nodepool.StoreOutputRequest parseFrom(byte[] data)
      throws com.google.protobuf.InvalidProtocolBufferException {
    return PARSER.parseFrom(data);
  }
  public static nodepool.StoreOutputRequest parseFrom(
      byte[] data,
      com.google.protobuf.ExtensionRegistryLite extensionRegistry)
      throws com.google.protobuf.InvalidProtocolBufferException {
    return PARSER.parseFrom(data, extensionRegistry);
  }
  public static nodepool.StoreOutputRequest parseFrom(java.io.InputStream input)
      throws java.io.IOException {
    return com.google.protobuf.GeneratedMessage
        .parseWithIOException(PARSER, input);
  }
  public static nodepool.StoreOutputRequest parseFrom(
      java.io.InputStream input,
      com.google.protobuf.ExtensionRegistryLite extensionRegistry)
      throws java.io.IOException {
    return com.google.protobuf.GeneratedMessage
        .parseWithIOException(PARSER, input, extensionRegistry);
  }

  public static nodepool.StoreOutputRequest parseDelimitedFrom(java.io.InputStream input)
      throws java.io.IOException {
    return com.google.protobuf.GeneratedMessage
        .parseDelimitedWithIOException(PARSER, input);
  }

  public static nodepool.StoreOutputRequest parseDelimitedFrom(
      java.io.InputStream input,
      com.google.protobuf.ExtensionRegistryLite extensionRegistry)
      throws java.io.IOException {
    return com.google.protobuf.GeneratedMessage
        .parseDelimitedWithIOException(PARSER, input, extensionRegistry);
  }
  public static nodepool.StoreOutputRequest parseFrom(
      com.google.protobuf.CodedInputStream input)
      throws java.io.IOException {
    return com.google.protobuf.GeneratedMessage
        .parseWithIOException(PARSER, input);
  }
  public static nodepool.StoreOutputRequest parseFrom(
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
  public static Builder newBuilder(nodepool.StoreOutputRequest prototype) {
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
   * Protobuf type {@code nodepool.StoreOutputRequest}
   */
  public static final class Builder extends
      com.google.protobuf.GeneratedMessage.Builder<Builder> implements
      // @@protoc_insertion_point(builder_implements:nodepool.StoreOutputRequest)
      nodepool.StoreOutputRequestOrBuilder {
    public static final com.google.protobuf.Descriptors.Descriptor
        getDescriptor() {
      return nodepool.NodepoolOuterClass.internal_static_nodepool_StoreOutputRequest_descriptor;
    }

    @java.lang.Override
    protected com.google.protobuf.GeneratedMessage.FieldAccessorTable
        internalGetFieldAccessorTable() {
      return nodepool.NodepoolOuterClass.internal_static_nodepool_StoreOutputRequest_fieldAccessorTable
          .ensureFieldAccessorsInitialized(
              nodepool.StoreOutputRequest.class, nodepool.StoreOutputRequest.Builder.class);
    }

    // Construct using nodepool.StoreOutputRequest.newBuilder()
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
      output_ = "";
      return this;
    }

    @java.lang.Override
    public com.google.protobuf.Descriptors.Descriptor
        getDescriptorForType() {
      return nodepool.NodepoolOuterClass.internal_static_nodepool_StoreOutputRequest_descriptor;
    }

    @java.lang.Override
    public nodepool.StoreOutputRequest getDefaultInstanceForType() {
      return nodepool.StoreOutputRequest.getDefaultInstance();
    }

    @java.lang.Override
    public nodepool.StoreOutputRequest build() {
      nodepool.StoreOutputRequest result = buildPartial();
      if (!result.isInitialized()) {
        throw newUninitializedMessageException(result);
      }
      return result;
    }

    @java.lang.Override
    public nodepool.StoreOutputRequest buildPartial() {
      nodepool.StoreOutputRequest result = new nodepool.StoreOutputRequest(this);
      if (bitField0_ != 0) { buildPartial0(result); }
      onBuilt();
      return result;
    }

    private void buildPartial0(nodepool.StoreOutputRequest result) {
      int from_bitField0_ = bitField0_;
      if (((from_bitField0_ & 0x00000001) != 0)) {
        result.taskId_ = taskId_;
      }
      if (((from_bitField0_ & 0x00000002) != 0)) {
        result.output_ = output_;
      }
    }

    @java.lang.Override
    public Builder mergeFrom(com.google.protobuf.Message other) {
      if (other instanceof nodepool.StoreOutputRequest) {
        return mergeFrom((nodepool.StoreOutputRequest)other);
      } else {
        super.mergeFrom(other);
        return this;
      }
    }

    public Builder mergeFrom(nodepool.StoreOutputRequest other) {
      if (other == nodepool.StoreOutputRequest.getDefaultInstance()) return this;
      if (!other.getTaskId().isEmpty()) {
        taskId_ = other.taskId_;
        bitField0_ |= 0x00000001;
        onChanged();
      }
      if (!other.getOutput().isEmpty()) {
        output_ = other.output_;
        bitField0_ |= 0x00000002;
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
              output_ = input.readStringRequireUtf8();
              bitField0_ |= 0x00000002;
              break;
            } // case 18
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

    private java.lang.Object output_ = "";
    /**
     * <code>string output = 2;</code>
     * @return The output.
     */
    public java.lang.String getOutput() {
      java.lang.Object ref = output_;
      if (!(ref instanceof java.lang.String)) {
        com.google.protobuf.ByteString bs =
            (com.google.protobuf.ByteString) ref;
        java.lang.String s = bs.toStringUtf8();
        output_ = s;
        return s;
      } else {
        return (java.lang.String) ref;
      }
    }
    /**
     * <code>string output = 2;</code>
     * @return The bytes for output.
     */
    public com.google.protobuf.ByteString
        getOutputBytes() {
      java.lang.Object ref = output_;
      if (ref instanceof String) {
        com.google.protobuf.ByteString b = 
            com.google.protobuf.ByteString.copyFromUtf8(
                (java.lang.String) ref);
        output_ = b;
        return b;
      } else {
        return (com.google.protobuf.ByteString) ref;
      }
    }
    /**
     * <code>string output = 2;</code>
     * @param value The output to set.
     * @return This builder for chaining.
     */
    public Builder setOutput(
        java.lang.String value) {
      if (value == null) { throw new NullPointerException(); }
      output_ = value;
      bitField0_ |= 0x00000002;
      onChanged();
      return this;
    }
    /**
     * <code>string output = 2;</code>
     * @return This builder for chaining.
     */
    public Builder clearOutput() {
      output_ = getDefaultInstance().getOutput();
      bitField0_ = (bitField0_ & ~0x00000002);
      onChanged();
      return this;
    }
    /**
     * <code>string output = 2;</code>
     * @param value The bytes for output to set.
     * @return This builder for chaining.
     */
    public Builder setOutputBytes(
        com.google.protobuf.ByteString value) {
      if (value == null) { throw new NullPointerException(); }
      checkByteStringIsUtf8(value);
      output_ = value;
      bitField0_ |= 0x00000002;
      onChanged();
      return this;
    }

    // @@protoc_insertion_point(builder_scope:nodepool.StoreOutputRequest)
  }

  // @@protoc_insertion_point(class_scope:nodepool.StoreOutputRequest)
  private static final nodepool.StoreOutputRequest DEFAULT_INSTANCE;
  static {
    DEFAULT_INSTANCE = new nodepool.StoreOutputRequest();
  }

  public static nodepool.StoreOutputRequest getDefaultInstance() {
    return DEFAULT_INSTANCE;
  }

  private static final com.google.protobuf.Parser<StoreOutputRequest>
      PARSER = new com.google.protobuf.AbstractParser<StoreOutputRequest>() {
    @java.lang.Override
    public StoreOutputRequest parsePartialFrom(
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

  public static com.google.protobuf.Parser<StoreOutputRequest> parser() {
    return PARSER;
  }

  @java.lang.Override
  public com.google.protobuf.Parser<StoreOutputRequest> getParserForType() {
    return PARSER;
  }

  @java.lang.Override
  public nodepool.StoreOutputRequest getDefaultInstanceForType() {
    return DEFAULT_INSTANCE;
  }

}

