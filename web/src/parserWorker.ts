/// <reference lib="webworker" />

import init, { parse_xml_content } from "../../crates/wasm/pkg/wasm_mojxml.js";
import type { InitOutput } from "../../crates/wasm/pkg/wasm_mojxml.js";
import type { FeatureCollection } from "geojson";

declare const self: DedicatedWorkerGlobalScope;

type ParseRequest = {
  fileName: string;
  xmlBytes: Uint8Array;
};

type WorkerMemorySnapshot = {
  wasmHeapBytes: number;
  jsHeapUsedBytes?: number;
  jsHeapTotalBytes?: number;
  jsHeapLimitBytes?: number;
};

type WorkerParseProfile = {
  xmlBytes: number;
  decodeMs: number;
  parseMs: number;
  totalMs: number;
  memoryBefore: WorkerMemorySnapshot;
  memoryAfter: WorkerMemorySnapshot;
};

type ParseSuccessResponse = {
  status: "success";
  result: FeatureCollection;
  elapsedMs: number;
  profile: WorkerParseProfile;
};

type ParseErrorResponse = {
  status: "error";
  message: string;
};

type ParseResponse = ParseSuccessResponse | ParseErrorResponse;

type PerformanceWithMemory = Performance & {
  memory?: {
    usedJSHeapSize: number;
    totalJSHeapSize: number;
    jsHeapSizeLimit: number;
  };
};

const textDecoder = new TextDecoder();

let initPromise: Promise<InitOutput> | undefined;
let wasmInit: InitOutput | undefined;

const ensureInit = () => {
  if (!initPromise) {
    initPromise = init().then((output) => {
      wasmInit = output;
      return output;
    });
  }
  return initPromise;
};

const captureMemorySnapshot = (): WorkerMemorySnapshot => {
  const perf = performance as PerformanceWithMemory;
  const performanceMemory = perf.memory;

  return {
    wasmHeapBytes: wasmInit?.memory.buffer.byteLength ?? 0,
    jsHeapUsedBytes: performanceMemory?.usedJSHeapSize,
    jsHeapTotalBytes: performanceMemory?.totalJSHeapSize,
    jsHeapLimitBytes: performanceMemory?.jsHeapSizeLimit,
  };
};

const normalizeFeatureCollection = (
  raw: unknown,
  fileName: string,
): FeatureCollection => {
  let candidate: unknown = raw;
  if (typeof raw === "string") {
    try {
      candidate = JSON.parse(raw);
    } catch (error) {
      throw new Error(
        `parse_xml_content returned invalid JSON string for ${fileName}: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
  }

  if (candidate instanceof Map) {
    const mapped: Record<string, unknown> = {};
    for (const [key, value] of candidate.entries()) {
      if (typeof key === "string") {
        mapped[key] = value;
      }
    }
    candidate = mapped;
  }

  if (!candidate || typeof candidate !== "object") {
    throw new Error(
      `parse_xml_content returned non-object for ${fileName} (type=${typeof candidate})`,
    );
  }

  const collection = candidate as {
    type?: unknown;
    features?: unknown;
  };
  if (collection.type !== undefined && collection.type !== "FeatureCollection") {
    throw new Error(
      `parse_xml_content returned unexpected GeoJSON type for ${fileName}: ${String(
        collection.type,
      )}`,
    );
  }
  if (!Array.isArray(collection.features)) {
    throw new Error(
      `parse_xml_content returned FeatureCollection without features[] for ${fileName}`,
    );
  }

  if (collection.type === "FeatureCollection") {
    return candidate as FeatureCollection;
  }

  return {
    ...(candidate as Record<string, unknown>),
    type: "FeatureCollection",
    features: collection.features,
  } as FeatureCollection;
};

self.addEventListener("message", async (event: MessageEvent<ParseRequest>) => {
  const { fileName, xmlBytes } = event.data;
  await ensureInit();

  try {
    const startedAt = performance.now();
    const memoryBefore = captureMemorySnapshot();

    const decodeStartedAt = performance.now();
    const xml = textDecoder.decode(xmlBytes);
    const decodeMs = performance.now() - decodeStartedAt;

    const parseStartedAt = performance.now();
    const rawResult = parse_xml_content(fileName, xml);
    const result = normalizeFeatureCollection(rawResult, fileName);
    const parseMs = performance.now() - parseStartedAt;
    const elapsedMs = performance.now() - startedAt;

    const response: ParseResponse = {
      status: "success",
      result,
      elapsedMs,
      profile: {
        xmlBytes: xmlBytes.byteLength,
        decodeMs,
        parseMs,
        totalMs: elapsedMs,
        memoryBefore,
        memoryAfter: captureMemorySnapshot(),
      },
    };
    self.postMessage(response);
  } catch (error) {
    let message = "Unknown error";
    if (error instanceof Error) {
      message = error.message;
    } else if (typeof error === "string") {
      message = error;
    } else {
      try {
        message = JSON.stringify(error);
      } catch (stringifyError) {
        message = `Failed to stringify error: ${String(stringifyError)}`;
      }
    }

    const response: ParseResponse = {
      status: "error",
      message,
    };
    self.postMessage(response);
  }
});

export {};
