import { mapLimit } from "async";
import * as maplibregl from "maplibre-gl";
import maplibreWorkerUrl from "maplibre-gl/dist/maplibre-gl-worker.mjs?worker&url";
import type { FeatureCollection, GeoJsonProperties } from "geojson";
import { unzip } from "fflate";
import "bootstrap/dist/css/bootstrap.min.css";
import "maplibre-gl/dist/maplibre-gl.css";

maplibregl.setWorkerUrl(maplibreWorkerUrl);

type WorkerSuccessMessage = {
  status: "success";
  result: FeatureCollection;
  elapsedMs: number;
  profile: WorkerParseProfile;
};

type WorkerErrorMessage = {
  status: "error";
  message: string;
};

type WorkerMessage = WorkerSuccessMessage | WorkerErrorMessage;

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

type ParseProfileSummary = {
  samples: number;
  maxXmlBytes: number;
  maxWasmHeapBytes: number;
};

const MAP_CONTAINER_ID = "map";
const COPYRIGHT_YEAR_ID = "copyright-year";
const GEOJSON_SOURCE_ID = "parsed-geojson";
const POLYGON_LAYER_ID = `${GEOJSON_SOURCE_ID}-polygon`;
const POLYGON_OUTLINE_LAYER_ID = `${GEOJSON_SOURCE_ID}-polygon-outline`;
const POLYGON_LABEL_LAYER_ID = `${GEOJSON_SOURCE_ID}-polygon-label`;
const BASE_STYLE_URL = "https://tiles.kmproj.com/styles/osm-ja-light.json";
const DEFAULT_PARSER_WORKER_CONCURRENCY = 2;
const MAX_PARSER_WORKER_CONCURRENCY = 4;
const WASM_WORKER_RECYCLE_THRESHOLD_BYTES = 192 * 1024 * 1024;
const ENABLE_PROFILE_LOGS = new URLSearchParams(window.location.search).has("profile");

const parseWorkerConcurrencyFromQuery = () => {
  const queryValue = new URLSearchParams(window.location.search).get("workers");
  if (!queryValue) {
    return Math.min(
      navigator.hardwareConcurrency || DEFAULT_PARSER_WORKER_CONCURRENCY,
      DEFAULT_PARSER_WORKER_CONCURRENCY,
    );
  }

  const requested = Number.parseInt(queryValue, 10);
  if (!Number.isFinite(requested) || requested < 1) {
    return Math.min(
      navigator.hardwareConcurrency || DEFAULT_PARSER_WORKER_CONCURRENCY,
      DEFAULT_PARSER_WORKER_CONCURRENCY,
    );
  }

  return Math.min(requested, MAX_PARSER_WORKER_CONCURRENCY);
};

const PARSER_WORKER_CONCURRENCY = Math.max(1, parseWorkerConcurrencyFromQuery());

class ParserWorkerPool {
  private idleWorkers: Worker[] = [];
  private pendingResolvers: Array<(worker: Worker) => void> = [];
  private totalWorkers = 0;
  private readonly maxSize: number;

  constructor() {
    this.maxSize = PARSER_WORKER_CONCURRENCY;
  }

  acquire(): Promise<Worker> {
    const idleWorker = this.idleWorkers.pop();
    if (idleWorker) {
      return Promise.resolve(idleWorker);
    }

    if (this.totalWorkers < this.maxSize) {
      this.totalWorkers += 1;
      return Promise.resolve(this.createWorker());
    }

    return new Promise<Worker>((resolve) => {
      this.pendingResolvers.push(resolve);
    });
  }

  release(worker: Worker) {
    const pending = this.pendingResolvers.shift();
    if (pending) {
      pending(worker);
      return;
    }

    this.idleWorkers.push(worker);
  }

  invalidate(worker: Worker) {
    worker.terminate();
    this.totalWorkers = Math.max(0, this.totalWorkers - 1);

    const pending = this.pendingResolvers.shift();
    if (!pending) {
      return;
    }

    this.totalWorkers += 1;
    pending(this.createWorker());
  }

  disposeIdleWorkers() {
    if (this.idleWorkers.length === 0) {
      return;
    }

    for (const worker of this.idleWorkers) {
      worker.terminate();
    }
    this.totalWorkers = Math.max(0, this.totalWorkers - this.idleWorkers.length);
    this.idleWorkers.length = 0;
  }

  private createWorker() {
    return new Worker(new URL("./parserWorker.ts", import.meta.url), {
      type: "module",
    });
  }
}

const parserWorkerPool = new ParserWorkerPool();

class FeatureInfoControl implements maplibregl.IControl {
  private container: HTMLDivElement | undefined;
  private messageEl: HTMLDivElement | undefined;
  private tableEl: HTMLTableElement | undefined;
  private tableBody: HTMLTableSectionElement | undefined;

  onAdd(_map: maplibregl.Map) {
    this.container = document.createElement("div");
    this.container.className = "maplibregl-ctrl feature-info-control";

    this.messageEl = document.createElement("div");
    this.messageEl.className = "feature-info-empty";

    this.tableEl = document.createElement("table");
    this.tableEl.className = "feature-info-table";
    this.tableBody = this.tableEl.createTBody();

    this.container.append(this.messageEl, this.tableEl);
    this.clear();

    return this.container;
  }

  onRemove() {
    this.container?.remove();
    this.container = undefined;
    this.messageEl = undefined;
    this.tableEl = undefined;
    this.tableBody = undefined;
  }

  setProperties(properties: GeoJsonProperties | undefined | null) {
    if (!properties || Object.keys(properties).length === 0) {
      this.clear();
      return;
    }

    if (!this.tableBody || !this.tableEl || !this.messageEl) {
      return;
    }

    this.messageEl.style.display = "none";
    this.tableEl.style.display = "";

    while (this.tableBody.firstChild) {
      this.tableBody.firstChild.remove();
    }

    for (const [key, value] of Object.entries(properties)) {
      const row = this.tableBody.insertRow();
      const keyCell = row.insertCell();
      keyCell.textContent = key;
      keyCell.className = "feature-info-key";

      const valueCell = row.insertCell();
      valueCell.textContent = this.formatValue(value);
      valueCell.className = "feature-info-value";
    }
  }

  clear() {
    if (!this.messageEl || !this.tableEl || !this.tableBody) {
      return;
    }

    while (this.tableBody.firstChild) {
      this.tableBody.firstChild.remove();
    }

    this.messageEl.textContent = "Hover over a feature to see its attributes.";
    this.messageEl.style.display = "";
    this.tableEl.style.display = "none";
  }

  private formatValue(value: unknown) {
    if (value === null) {
      return "null";
    }
    if (value === undefined) {
      return "undefined";
    }
    if (typeof value === "object") {
      try {
        return JSON.stringify(value);
      } catch (error) {
        console.warn("Failed to stringify property value:", error);
        return "[object]";
      }
    }
    return String(value);
  }
}

const HOVER_TARGET_LAYER_IDS = [
  POLYGON_LAYER_ID,
  POLYGON_OUTLINE_LAYER_ID,
  POLYGON_LABEL_LAYER_ID,
] as const;

let featureInfoControl: FeatureInfoControl | undefined;
const registeredHoverLayers = new Set<string>();
let stickyFeatureId: string | number | undefined;
let backgroundClickHandlerRegistered = false;

function getFeatureIdentifier(feature: maplibregl.MapGeoJSONFeature) {
  if (feature.id !== undefined && feature.id !== null) {
    return feature.id as string | number;
  }

  const properties = (feature.properties ?? undefined) as GeoJsonProperties | undefined;
  const banChi = properties?.["番地"];
  if (typeof banChi === "string" || typeof banChi === "number") {
    return banChi;
  }

  return properties ? JSON.stringify(properties) : undefined;
}

function registerHoverInteraction(map: maplibregl.Map, layerId: string) {
  if (registeredHoverLayers.has(layerId)) {
    return;
  }

  registeredHoverLayers.add(layerId);

  map.on("mouseenter", layerId, () => {
    map.getCanvas().style.cursor = "pointer";
  });

  map.on("mousemove", layerId, (event) => {
    const feature = event.features?.[0];
    if (!feature) {
      return;
    }

    const featureId = getFeatureIdentifier(feature);
    if (stickyFeatureId !== undefined && featureId !== stickyFeatureId) {
      return;
    }

    featureInfoControl?.setProperties(
      (feature.properties as GeoJsonProperties | undefined) ?? null,
    );
  });

  map.on("mouseleave", layerId, () => {
    map.getCanvas().style.cursor = "";
    if (stickyFeatureId === undefined) {
      featureInfoControl?.clear();
    }
  });

  map.on("click", layerId, (event) => {
    const feature = event.features?.[0];
    if (!feature) {
      return;
    }

    stickyFeatureId = getFeatureIdentifier(feature);
    featureInfoControl?.setProperties(
      (feature.properties as GeoJsonProperties | undefined) ?? null,
    );
  });
}

let mapInstance: maplibregl.Map | undefined;

type XmlDocument = {
  fileName: string;
  xmlBytes: Uint8Array;
};

type ParsedDocument = {
  fileName: string;
  featureCount: number;
  result: WorkerSuccessMessage;
};

type FailedDocument = {
  fileName: string;
  error: unknown;
};

const ZIP_MIME_TYPES = new Set([
  "application/zip",
  "application/x-zip-compressed",
  "application/x-zip",
  "multipart/x-zip",
]);

const ZIP_EXTENSION = ".zip";
const XML_EXTENSION = ".xml";

const describeError = (error: unknown) => {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  if (error === undefined) {
    return "undefined";
  }
  try {
    const json = JSON.stringify(error);
    return json ?? String(error);
  } catch (jsonError) {
    console.warn("Failed to stringify error description:", jsonError);
    return String(error);
  }
};

const formatBytesMiB = (bytes?: number) => {
  if (bytes === undefined || bytes <= 0) {
    return "n/a";
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
};

const shouldRecycleWorker = (message: WorkerSuccessMessage) =>
  message.profile.memoryAfter.wasmHeapBytes >= WASM_WORKER_RECYCLE_THRESHOLD_BYTES;

const isZipFile = (file: File) => {
  const name = file.name.toLowerCase();
  return name.endsWith(ZIP_EXTENSION) || ZIP_MIME_TYPES.has(file.type);
};

const normalizeZipEntryName = (name: string) => name.replaceAll("\\", "/");

const unzipToObject = (data: Uint8Array) =>
  new Promise<Record<string, Uint8Array>>((resolve, reject) => {
    unzip(data, (error, result) => {
      if (error) {
        reject(error);
        return;
      }
      resolve(result);
    });
  });

async function extractXmlDocumentsFromZip(
  bytes: Uint8Array,
  originPath: string,
  log?: (message: string) => void,
): Promise<XmlDocument[]> {
  const xmlDocuments: XmlDocument[] = [];
  let entries: Record<string, Uint8Array>;

  try {
    entries = await unzipToObject(bytes);
  } catch (error) {
    console.error(`Failed to unzip ${originPath}:`, error);
    throw error;
  }

  for (const [rawName, content] of Object.entries(entries)) {
    // Drop references in the unzip object as soon as each entry is handled.
    delete entries[rawName];

    const normalizedName = normalizeZipEntryName(rawName);
    if (!normalizedName || normalizedName.endsWith("/")) {
      continue;
    }

    const fullPath = `${originPath}/${normalizedName}`;
    const lowerName = normalizedName.toLowerCase();

    if (lowerName.endsWith(ZIP_EXTENSION)) {
      try {
        const nestedDocuments = await extractXmlDocumentsFromZip(content, fullPath, log);
        xmlDocuments.push(...nestedDocuments);
      } catch (error) {
        console.error(`Failed to process nested archive ${fullPath}:`, error);
      }
      continue;
    }

    if (lowerName.endsWith(XML_EXTENSION)) {
      try {
        log?.(`unzipped ${fullPath}`);
        xmlDocuments.push({ fileName: fullPath, xmlBytes: content });
      } catch (error) {
        console.error(`Failed to queue XML document ${fullPath}:`, error);
      }
    }
  }

  return xmlDocuments;
}

const parseXmlWithWorker = async (
  fileName: string,
  xmlBytes: Uint8Array,
): Promise<WorkerSuccessMessage> => {
  const parserWorker = await parserWorkerPool.acquire();

  return new Promise<WorkerSuccessMessage>((resolve, reject) => {
    const cleanup = () => {
      parserWorker.removeEventListener("message", handleMessage);
      parserWorker.removeEventListener("error", handleError);
    };

    const handleMessage = (event: MessageEvent<WorkerMessage>) => {
      cleanup();
      if (event.data.status === "success") {
        if (shouldRecycleWorker(event.data)) {
          parserWorkerPool.invalidate(parserWorker);
        } else {
          parserWorkerPool.release(parserWorker);
        }
        resolve(event.data);
      } else {
        parserWorkerPool.release(parserWorker);
        reject(new Error(event.data.message));
      }
    };

    const handleError = (event: ErrorEvent) => {
      cleanup();
      parserWorkerPool.invalidate(parserWorker);
      reject(event.error ?? new Error(event.message));
    };

    parserWorker.addEventListener("message", handleMessage);
    parserWorker.addEventListener("error", handleError);

    const transferableBytes =
      xmlBytes.byteOffset === 0 && xmlBytes.byteLength === xmlBytes.buffer.byteLength
        ? xmlBytes
        : xmlBytes.slice();

    parserWorker.postMessage({
      fileName,
      xmlBytes: transferableBytes,
    }, [transferableBytes.buffer]);
  });
};

const parseXmlDocumentsWithWorker = async (
  entries: XmlDocument[],
  callbacks?: {
    onSuccess?: (parsed: ParsedDocument) => void;
    onFailure?: (failed: FailedDocument) => void;
  },
) => {
  const startTime = performance.now();
  const failures: FailedDocument[] = [];
  const mergedFeatures: FeatureCollection["features"] = [];
  let successCount = 0;
  let totalFeatures = 0;
  const profileSummary: ParseProfileSummary = {
    samples: 0,
    maxXmlBytes: 0,
    maxWasmHeapBytes: 0,
  };

  await mapLimit(entries, PARSER_WORKER_CONCURRENCY, async (entry: XmlDocument) => {
    try {
      const result = await parseXmlWithWorker(entry.fileName, entry.xmlBytes);
      const features = result.result.features;
      if (!Array.isArray(features)) {
        throw new Error(
          `Worker returned malformed FeatureCollection for ${entry.fileName} (features is not an array)`,
        );
      }

      const featureCount = features.length;
      for (const feature of features) {
        mergedFeatures.push(feature);
      }
      features.length = 0;

      successCount += 1;
      totalFeatures += featureCount;
      profileSummary.samples += 1;
      profileSummary.maxXmlBytes = Math.max(profileSummary.maxXmlBytes, result.profile.xmlBytes);
      profileSummary.maxWasmHeapBytes = Math.max(
        profileSummary.maxWasmHeapBytes,
        result.profile.memoryAfter.wasmHeapBytes,
      );

      const success: ParsedDocument = {
        fileName: entry.fileName,
        featureCount,
        result,
      };
      if (ENABLE_PROFILE_LOGS) {
        console.debug("parse-profile", success.fileName, success.result.profile);
      }
      callbacks?.onSuccess?.(success);
    } catch (error) {
      const failure: FailedDocument = { fileName: entry.fileName, error };
      failures.push(failure);
      callbacks?.onFailure?.(failure);
    } finally {
      // Release large buffers as soon as they are no longer needed.
      entry.xmlBytes = new Uint8Array(0);
    }

    return undefined;
  });

  const totalElapsedMs = performance.now() - startTime;
  const collection =
    successCount > 0
      ? ({
          type: "FeatureCollection",
          features: mergedFeatures,
        } as FeatureCollection)
      : undefined;

  return {
    collection,
    failures,
    successCount,
    totalFeatures,
    totalElapsedMs,
    profileSummary,
  };
};

async function main() {
  const dropArea = document.getElementById("drop-area");
  const copyrightYearEl = document.getElementById(COPYRIGHT_YEAR_ID);
  const statusEl = document.getElementById("status");
  const fileInput = document.getElementById("file-input") as HTMLInputElement | null;
  const downloadButton = document.getElementById(
    "download-geojson",
  ) as HTMLButtonElement | null;
  const testDataButton = document.getElementById("load-test-data") as HTMLButtonElement | null;
  const testDataOneFileButton = document.getElementById(
    "load-test-data-one-file",
  ) as HTMLButtonElement | null;

  if (copyrightYearEl) {
    copyrightYearEl.textContent = String(new Date().getFullYear());
  }

  initMap();

  if (!dropArea || !statusEl || !fileInput) {
    console.error("Required UI elements are missing from the page.");
    return;
  }

  let latestGeoJson: FeatureCollection | undefined;
  let latestDownloadFileName = "converted.geojson";

  const deriveGeoJsonFileName = (sourceName: string) => {
    const sanitized = sourceName.trim().replace(/[/\\?%*:|"<>]/g, "_");
    const baseName = sanitized.replace(/\.[^.]+$/, "") || "converted";
    return `${baseName}.geojson`;
  };

  const resetGeoJsonDownload = () => {
    latestGeoJson = undefined;
    latestDownloadFileName = "converted.geojson";
    if (downloadButton) {
      downloadButton.disabled = true;
    }
  };

  const prepareGeoJsonDownload = (collection: FeatureCollection, originName: string) => {
    latestGeoJson = collection;
    latestDownloadFileName = deriveGeoJsonFileName(originName);
    if (downloadButton) {
      downloadButton.disabled = false;
    }
  };

  downloadButton?.addEventListener("click", () => {
    if (!latestGeoJson) {
      return;
    }

    try {
      const blob = new Blob([JSON.stringify(latestGeoJson, null, 2)], {
        type: "application/geo+json",
      });
      const url = URL.createObjectURL(blob);
      const anchor = document.createElement("a");
      anchor.href = url;
      anchor.download = latestDownloadFileName;
      document.body.appendChild(anchor);
      anchor.click();
      document.body.removeChild(anchor);
      setTimeout(() => URL.revokeObjectURL(url), 0);
    } catch (error) {
      console.error("Failed to prepare GeoJSON download:", error);
    }
  });

  resetGeoJsonDownload();

  const statusLines: string[] = [];

  const renderStatus = () => {
    statusEl.textContent = statusLines.join("\n");
    statusEl.scrollTop = statusEl.scrollHeight;
  };

  const resetStatus = (message: string) => {
    statusLines.length = 0;
    statusLines.push(message);
    renderStatus();
  };

  const appendStatus = (message: string) => {
    statusLines.push(message);
    renderStatus();
  };

  const replaceLastStatus = (message: string) => {
    if (statusLines.length === 0) {
      statusLines.push(message);
    } else {
      statusLines[statusLines.length - 1] = message;
    }
    renderStatus();
  };

  const summarizeResults = (
    successCount: number,
    failures: FailedDocument[],
    totalFeatures: number,
    totalElapsedMs: number,
  ) => {
    if (successCount === 0) {
      const failureMessages = failures
        .map((failure) => `${failure.fileName}: ${describeError(failure.error)}`)
        .join(" • ");
      return failures.length === 0
        ? "No XML documents were processed."
        : `Failed to parse XML • ${failureMessages}`;
    }

    const summaryParts = [
      `Parsed ${successCount} XML file${successCount === 1 ? "" : "s"}`,
      `Total ${totalFeatures} feature${totalFeatures === 1 ? "" : "s"}`,
    ];

    if (totalElapsedMs > 0) {
      summaryParts.push(`Elapsed ${totalElapsedMs.toFixed(2)} ms`);
    }

    if (failures.length > 0) {
      const failureSummary = failures
        .map((failure) => `${failure.fileName}: ${describeError(failure.error)}`)
        .join(" • ");
      summaryParts.push(`Failed ${failures.length} • ${failureSummary}`);
    }

    return summaryParts.join(" • ");
  };

  const handleXmlDocuments = async (documents: XmlDocument[], originName: string) => {
    if (documents.length === 0) {
      appendStatus(`No XML documents found in ${originName}.`);
      return;
    }

    appendStatus(
      `Parsing ${documents.length} XML file${documents.length === 1 ? "" : "s"} from ${originName}...`,
    );

    const {
      collection,
      failures,
      successCount,
      totalFeatures,
      totalElapsedMs,
      profileSummary,
    } = await parseXmlDocumentsWithWorker(documents, {
      onSuccess: (parsed) => {
        appendStatus(
          `Processed ${parsed.fileName}: ${parsed.featureCount} feature${
            parsed.featureCount === 1 ? "" : "s"
          } in ${parsed.result.elapsedMs.toFixed(2)} ms`,
        );
      },
      onFailure: (failed) => {
        appendStatus(`Failed ${failed.fileName}: ${describeError(failed.error)}`);
      },
    });

    const message = summarizeResults(successCount, failures, totalFeatures, totalElapsedMs);

    if (collection) {
      updateMapWithGeoJson(collection);
      prepareGeoJsonDownload(collection, originName);
    }

    if (profileSummary.samples > 0) {
      appendStatus(
        `Peak worker memory • XML ${formatBytesMiB(profileSummary.maxXmlBytes)} • WASM ${formatBytesMiB(
          profileSummary.maxWasmHeapBytes,
        )}`,
      );
    }

    appendStatus(message);
    parserWorkerPool.disposeIdleWorkers();
  };

  const handleFile = async (file: File) => {
    resetGeoJsonDownload();
    resetStatus(`Reading ${file.name}...`);
    try {
      if (isZipFile(file)) {
        let archiveBytes = new Uint8Array(await file.arrayBuffer());
        replaceLastStatus(`Read ${file.name} (${archiveBytes.byteLength} bytes); extracting...`);
        const documents = await extractXmlDocumentsFromZip(
          archiveBytes,
          file.name,
          appendStatus,
        );
        archiveBytes = new Uint8Array(0);
        await handleXmlDocuments(documents, file.name);
        documents.length = 0;
        return;
      }

      const xmlBytes = new Uint8Array(await file.arrayBuffer());
      replaceLastStatus(`Read ${file.name} (${xmlBytes.byteLength} bytes).`);
      await handleXmlDocuments([{ fileName: file.name, xmlBytes }], file.name);
    } catch (error) {
      console.error("Failed to process file:", error);
      appendStatus(`Error processing ${file.name}: ${describeError(error)}`);
    }
  };

  const loadTestData = async (
    button: HTMLButtonElement,
    fileName: string,
    importTestDataUrl: () => Promise<{ default: string }>,
  ) => {
    button.disabled = true;
    const revertText = button.textContent;
    button.textContent = "読み込み中...";

    try {
      resetStatus("テストデータをダウンロードしています...");
      const testDataUrl = await importTestDataUrl();
      const response = await fetch(testDataUrl.default);
      if (!response.ok) {
        throw new Error(`Failed to fetch test data (${response.status} ${response.statusText})`);
      }

      const blob = await response.blob();
      const file = new File([blob], fileName, { type: "application/zip" });
      await handleFile(file);
    } catch (error) {
      console.error("Failed to load test data:", error);
      appendStatus(`テストデータの読み込みに失敗しました: ${describeError(error)}`);
    } finally {
      button.disabled = false;
      if (revertText !== null) {
        button.textContent = revertText;
      }
    }
  };

  testDataButton?.addEventListener("click", () => {
    void loadTestData(testDataButton, "46505-3411-2025.zip", () =>
      import(`../../testdata/46505-3411-2025.zip?url`),
    );
  });

  testDataOneFileButton?.addEventListener("click", () => {
    void loadTestData(testDataOneFileButton, "46505-3411-56.zip", () =>
      import(`../../testdata/46505-3411-56.zip?url`),
    );
  });

  const preventDefaults = (event: Event) => {
    event.preventDefault();
    event.stopPropagation();
  };

  const isFileDrag = (event: DragEvent) => {
    const types = event.dataTransfer?.types;
    if (!types) {
      return false;
    }
    if (typeof types.includes === "function") {
      return types.includes("Files");
    }
    return Array.from(types).includes("Files");
  };

  const addHighlight = () => dropArea.classList.add("highlight");
  const removeHighlight = () => dropArea.classList.remove("highlight");
  let windowDragDepth = 0;

  const eventTargets: (HTMLElement | Document)[] = [dropArea, document];
  ["dragenter", "dragover", "dragleave", "drop"].forEach((eventName) => {
    for (const target of eventTargets) {
      target.addEventListener(eventName, preventDefaults);
    }
  });

  ["dragenter", "dragover"].forEach((eventName) => {
    dropArea.addEventListener(eventName, () => addHighlight());
  });

  document.addEventListener("dragenter", (event) => {
    const dragEvent = event as DragEvent;
    if (!isFileDrag(dragEvent)) {
      return;
    }
    windowDragDepth += 1;
    addHighlight();
  });

  document.addEventListener("dragleave", (event) => {
    const dragEvent = event as DragEvent;
    if (!isFileDrag(dragEvent)) {
      return;
    }
    windowDragDepth = Math.max(windowDragDepth - 1, 0);
    if (windowDragDepth === 0) {
      removeHighlight();
    }
  });

  document.addEventListener("dragend", () => {
    windowDragDepth = 0;
    removeHighlight();
  });

  const handleDrop = (event: DragEvent) => {
    preventDefaults(event);
    windowDragDepth = 0;
    removeHighlight();
    const file = event.dataTransfer?.files?.[0];
    if (!file) {
      resetStatus("No file detected. Please drop an XML file.");
      return;
    }
    void handleFile(file);
  };

  dropArea.addEventListener("drop", (event) => handleDrop(event as DragEvent));
  document.addEventListener("drop", (event) => {
    handleDrop(event as DragEvent);
  });

  dropArea.addEventListener("click", () => {
    fileInput.click();
  });

  dropArea.addEventListener("keydown", (event) => {
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      fileInput.click();
    }
  });

  fileInput.addEventListener("change", () => {
    const file = fileInput.files?.[0];
    if (file) {
      void handleFile(file);
      fileInput.value = "";
    }
  });

  resetStatus("Drop an XML file above to parse it.");
}

void main();

function initMap() {
  if (mapInstance) {
    return mapInstance;
  }

  const container = document.getElementById(MAP_CONTAINER_ID);
  if (!container) {
    console.warn(`Map container #${MAP_CONTAINER_ID} not found. Skipping map init.`);
    return undefined;
  }

  mapInstance = new maplibregl.Map({
    container: MAP_CONTAINER_ID,
    style: BASE_STYLE_URL,
    center: [138.25, 36.2],
    zoom: 5,
    localIdeographFontFamily: "sans-serif",
  });
  mapInstance.addControl(new maplibregl.NavigationControl(), "top-right");
  mapInstance.addControl(new maplibregl.ScaleControl(), "bottom-left");
  featureInfoControl ??= new FeatureInfoControl();
  mapInstance.addControl(featureInfoControl, "top-left");
  return mapInstance;
}

function updateMapWithGeoJson(collection: FeatureCollection) {
  const map = initMap();
  if (!map) {
    return;
  }

  featureInfoControl?.clear();
  stickyFeatureId = undefined;

  const applyGeoJson = () => {
    let source = map.getSource(GEOJSON_SOURCE_ID) as maplibregl.GeoJSONSource | undefined;
    if (source) {
      source.setData(collection);
    } else {
      map.addSource(GEOJSON_SOURCE_ID, {
        type: "geojson",
        data: collection,
        generateId: true,
      });
      source = map.getSource(GEOJSON_SOURCE_ID) as maplibregl.GeoJSONSource | undefined;

      if (!map.getLayer(POLYGON_LAYER_ID)) {
        map.addLayer({
          id: POLYGON_LAYER_ID,
          type: "fill",
          source: GEOJSON_SOURCE_ID,
          paint: {
            "fill-color": "#397ac3",
            "fill-opacity": 0.25,
          },
          filter: ["in", "$type", "Polygon"],
        });
      }

      if (!map.getLayer(POLYGON_OUTLINE_LAYER_ID)) {
        map.addLayer({
          id: POLYGON_OUTLINE_LAYER_ID,
          type: "line",
          source: GEOJSON_SOURCE_ID,
          paint: {
            "line-color": "#1f5a96",
            "line-width": 2,
          },
          filter: ["in", "$type", "Polygon"],
        });
      }

      if (!map.getLayer(POLYGON_LABEL_LAYER_ID)) {
        map.addLayer({
          id: POLYGON_LABEL_LAYER_ID,
          type: "symbol",
          source: GEOJSON_SOURCE_ID,
          layout: {
            "text-field": ["to-string", ["get", "地番"]],
            "text-size": 12,
            "text-allow-overlap": false,
            "text-font": ["Noto Sans Regular"],
            "symbol-placement": "point",
          },
          paint: {
            "text-color": "#1f2f3d",
            "text-halo-color": "#ffffff",
            "text-halo-width": 1.5,
          },
          filter: ["in", "$type", "Polygon"],
        });
      }
    }

    for (const layerId of HOVER_TARGET_LAYER_IDS) {
      if (map.getLayer(layerId)) {
        registerHoverInteraction(map, layerId);
      }
    }

    if (!backgroundClickHandlerRegistered) {
      map.on("click", (event) => {
        const features = map.queryRenderedFeatures(event.point, {
          layers: [...HOVER_TARGET_LAYER_IDS],
        });

        if (features.length === 0) {
          stickyFeatureId = undefined;
          featureInfoControl?.clear();
        }
      });
      backgroundClickHandlerRegistered = true;
    }

    void source
      ?.getBounds()
      .then((bounds) => {
        if (!bounds.isEmpty()) {
          map.fitBounds(bounds, { padding: 32, maxZoom: 14 });
        }
      })
      .catch((error) => {
        console.warn("Failed to derive bounds from GeoJSON source:", error);
      });
  };

  if (map.isStyleLoaded()) {
    applyGeoJson();
  } else {
    map.once("load", applyGeoJson);
  }
}
