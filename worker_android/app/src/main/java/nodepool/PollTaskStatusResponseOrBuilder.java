// Generated by the protocol buffer compiler.  DO NOT EDIT!
// NO CHECKED-IN PROTOBUF GENCODE
// source: nodepool.proto
// Protobuf Java Version: 4.31.1

package nodepool;

@com.google.protobuf.Generated
public interface PollTaskStatusResponseOrBuilder extends
    // @@protoc_insertion_point(interface_extends:nodepool.PollTaskStatusResponse)
    com.google.protobuf.MessageOrBuilder {

  /**
   * <pre>
   * Fixed typo from 'рука_id'
   * </pre>
   *
   * <code>string task_id = 1;</code>
   * @return The taskId.
   */
  java.lang.String getTaskId();
  /**
   * <pre>
   * Fixed typo from 'рука_id'
   * </pre>
   *
   * <code>string task_id = 1;</code>
   * @return The bytes for taskId.
   */
  com.google.protobuf.ByteString
      getTaskIdBytes();

  /**
   * <code>string status = 2;</code>
   * @return The status.
   */
  java.lang.String getStatus();
  /**
   * <code>string status = 2;</code>
   * @return The bytes for status.
   */
  com.google.protobuf.ByteString
      getStatusBytes();

  /**
   * <code>repeated string output = 3;</code>
   * @return A list containing the output.
   */
  java.util.List<java.lang.String>
      getOutputList();
  /**
   * <code>repeated string output = 3;</code>
   * @return The count of output.
   */
  int getOutputCount();
  /**
   * <code>repeated string output = 3;</code>
   * @param index The index of the element to return.
   * @return The output at the given index.
   */
  java.lang.String getOutput(int index);
  /**
   * <code>repeated string output = 3;</code>
   * @param index The index of the value to return.
   * @return The bytes of the output at the given index.
   */
  com.google.protobuf.ByteString
      getOutputBytes(int index);

  /**
   * <code>string message = 4;</code>
   * @return The message.
   */
  java.lang.String getMessage();
  /**
   * <code>string message = 4;</code>
   * @return The bytes for message.
   */
  com.google.protobuf.ByteString
      getMessageBytes();
}
