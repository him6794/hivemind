export interface RoutingOptions {
  mode?: 'auto' | 'manual' | 'single' | 'distributed';
  targetWorker?: string;
  targetWorkers?: string[];
  deviceTag?: string;
  hardware?: 'cpu' | 'gpu' | 'npu';
  ownDevicesOnly?: boolean;
  trustTier?: 'tier0_private' | 'tier1_public' | 'tier2_tee';
  overflowPolicy?: 'strict_private' | 'prefer_private_then_public';
}

export class HivemindError extends Error {
  status: number;
  code: string | null;
  constructor(status: number, message: string, code?: string | null);
}

export class HivemindClient {
  baseUrl: string;
  constructor(baseUrl: string, token: string, opts?: { fetchImpl?: typeof fetch });
  listModels(): Promise<Array<Record<string, unknown>>>;
  chatCompletions(req: {
    model: string;
    messages: Array<{ role: string; content: string }>;
    maxTokens?: number;
    temperature?: number;
    topP?: number;
    stop?: string[];
    routing?: RoutingOptions;
  }): Promise<{ data: Record<string, unknown>; headers: Record<string, string | null> }>;
  streamChatCompletions(req: {
    model: string;
    messages: Array<{ role: string; content: string }>;
    maxTokens?: number;
    temperature?: number;
    topP?: number;
    stop?: string[];
    routing?: RoutingOptions;
  }): AsyncGenerator<Record<string, unknown>>;
  completions(req: {
    model: string;
    prompt: string;
    maxTokens?: number;
    temperature?: number;
    routing?: RoutingOptions;
  }): Promise<{ data: Record<string, unknown>; headers: Record<string, string | null> }>;
  submitWasmTask(req: {
    taskId: string;
    wasmBase64: string;
    entrypoint?: string;
    inputJson?: string;
    maxFuel?: number;
    maxMemoryPages?: number;
    maxCpt?: number;
  }): Promise<Record<string, unknown>>;
  createJob(req: {
    name?: string;
    taskSource: string;
    inputs: unknown[];
    perItemMaxCpt: number;
    jobMaxCpt?: number;
  }): Promise<Record<string, unknown>>;
  createDagPipeline(req: {
    name: string;
    budgetCpt: number;
    nodes: Array<Record<string, unknown>>;
  }): Promise<Record<string, unknown>>;
  getDagPipeline(pipelineId: string): Promise<Record<string, unknown>>;
  cancelDagPipeline(pipelineId: string): Promise<Record<string, unknown>>;
  listDevices(): Promise<Array<Record<string, unknown>>>;
  networkStatus(): Promise<Record<string, unknown>>;
}
