// Generated by the protocol buffer compiler.  DO NOT EDIT!
// NO CHECKED-IN PROTOBUF GENCODE
// source: nodepool.proto
// Protobuf Java Version: 4.31.1

package nodepool;

@com.google.protobuf.Generated
public interface GetTaskResultRequestOrBuilder extends
    // @@protoc_insertion_point(interface_extends:nodepool.GetTaskResultRequest)
    com.google.protobuf.MessageOrBuilder {

  /**
   * <code>string task_id = 1;</code>
   * @return The taskId.
   */
  java.lang.String getTaskId();
  /**
   * <code>string task_id = 1;</code>
   * @return The bytes for taskId.
   */
  com.google.protobuf.ByteString
      getTaskIdBytes();

  /**
   * <pre>
   * 用戶令牌，用於驗證
   * </pre>
   *
   * <code>string token = 2;</code>
   * @return The token.
   */
  java.lang.String getToken();
  /**
   * <pre>
   * 用戶令牌，用於驗證
   * </pre>
   *
   * <code>string token = 2;</code>
   * @return The bytes for token.
   */
  com.google.protobuf.ByteString
      getTokenBytes();
}
