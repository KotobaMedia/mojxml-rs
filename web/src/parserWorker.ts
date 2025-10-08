/// <reference lib="webworker" />

import init, { parse_xml_content } from "../../crates/wasm/pkg/wasm_mojxml.js";
import type { FeatureCollection } from "geojson";

declare const self: DedicatedWorkerGlobalScope;

type ParseRequest = {
  fileName: string;
  xml: string;
};

type ParseSuccessResponse = {
  status: "success";
  result: FeatureCollection;
  elapsedMs: number;
};

type ParseErrorResponse = {
  status: "error";
  message: string;
};

type ParseResponse = ParseSuccessResponse | ParseErrorResponse;

let initPromise: Promise<unknown> | undefined;

const ensureInit = () => {
  if (!initPromise) {
    initPromise = init();
  }
  return initPromise;
};

self.addEventListener("message", async (event: MessageEvent<ParseRequest>) => {
  const { fileName, xml } = event.data;
  await ensureInit();

  try {
    const start = performance.now();
    const result = parse_xml_content(fileName, xml);
    const elapsedMs = performance.now() - start;
    const response: ParseResponse = {
      status: "success",
      result: JSON.parse(result) as FeatureCollection,
      elapsedMs,
    };
    self.postMessage(response);
  } catch (error) {
    const message =
      error instanceof Error
        ? error.message
        : typeof error === "string"
        ? error
        : JSON.stringify(error);
    const response: ParseResponse = {
      status: "error",
      message,
    };
    self.postMessage(response);
  }
});

export {};
