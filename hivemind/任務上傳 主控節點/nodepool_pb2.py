# -*- coding: utf-8 -*-
# Generated by the protocol buffer compiler.  DO NOT EDIT!
# NO CHECKED-IN PROTOBUF GENCODE
# source: nodepool.proto
# Protobuf Python Version: 5.27.2
"""Generated protocol buffer code."""
from google.protobuf import descriptor as _descriptor
from google.protobuf import descriptor_pool as _descriptor_pool
from google.protobuf import runtime_version as _runtime_version
from google.protobuf import symbol_database as _symbol_database
from google.protobuf.internal import builder as _builder
_runtime_version.ValidateProtobufRuntimeVersion(
    _runtime_version.Domain.PUBLIC,
    5,
    27,
    2,
    '',
    'nodepool.proto'
)
# @@protoc_insertion_point(imports)

_sym_db = _symbol_database.Default()




DESCRIPTOR = _descriptor_pool.Default().AddSerializedFile(b'\n\x0enodepool.proto\x12\x08nodepool\"|\n\x04Node\x12\n\n\x02ip\x18\x01 \x01(\t\x12\x11\n\tcpu_score\x18\x02 \x01(\x05\x12\x11\n\tgpu_score\x18\x03 \x01(\x05\x12\x0e\n\x06memory\x18\x04 \x01(\x05\x12\x15\n\rnetwork_delay\x18\x05 \x01(\x05\x12\x1b\n\x13geographic_location\x18\x06 \x01(\t\"`\n\x0bNodeRequest\x12\x11\n\tcpu_score\x18\x01 \x01(\x05\x12\x11\n\tgpu_score\x18\x02 \x01(\x05\x12\x0e\n\x06memory\x18\x03 \x01(\x05\x12\x1b\n\x13geographic_location\x18\x04 \x01(\t\"#\n\x10RegisterResponse\x12\x0f\n\x07message\x18\x01 \x01(\t\"+\n\rUpdateRequest\x12\n\n\x02ip\x18\x01 \x01(\t\x12\x0e\n\x06status\x18\x02 \x01(\t\"!\n\x0eUpdateResponse\x12\x0f\n\x07message\x18\x01 \x01(\t2\xb3\x01\n\x08NodePool\x12\x36\n\x08Register\x12\x0e.nodepool.Node\x1a\x1a.nodepool.RegisterResponse\x12,\n\x03Get\x12\x15.nodepool.NodeRequest\x1a\x0e.nodepool.Node\x12\x41\n\x0cUpdateStatus\x12\x17.nodepool.UpdateRequest\x1a\x18.nodepool.UpdateResponseb\x06proto3')

_globals = globals()
_builder.BuildMessageAndEnumDescriptors(DESCRIPTOR, _globals)
_builder.BuildTopDescriptorsAndMessages(DESCRIPTOR, 'nodepool_pb2', _globals)
if not _descriptor._USE_C_DESCRIPTORS:
  DESCRIPTOR._loaded_options = None
  _globals['_NODE']._serialized_start=28
  _globals['_NODE']._serialized_end=152
  _globals['_NODEREQUEST']._serialized_start=154
  _globals['_NODEREQUEST']._serialized_end=250
  _globals['_REGISTERRESPONSE']._serialized_start=252
  _globals['_REGISTERRESPONSE']._serialized_end=287
  _globals['_UPDATEREQUEST']._serialized_start=289
  _globals['_UPDATEREQUEST']._serialized_end=332
  _globals['_UPDATERESPONSE']._serialized_start=334
  _globals['_UPDATERESPONSE']._serialized_end=367
  _globals['_NODEPOOL']._serialized_start=370
  _globals['_NODEPOOL']._serialized_end=549
# @@protoc_insertion_point(module_scope)
