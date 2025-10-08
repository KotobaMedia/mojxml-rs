import maplibregl from "maplibre-gl";
import type { FeatureCollection, GeoJsonProperties } from "geojson";
import "maplibre-gl/dist/maplibre-gl.css";

type WorkerSuccessMessage = {
  id: number;
  status: "success";
  result: FeatureCollection;
  elapsedMs: number;
};

type WorkerErrorMessage = {
  id: number;
  status: "error";
  message: string;
};

type WorkerMessage = WorkerSuccessMessage | WorkerErrorMessage;

const parserWorker = new Worker(new URL("./parserWorker.ts", import.meta.url), {
  type: "module",
});

const MAP_CONTAINER_ID = "map";
const GEOJSON_SOURCE_ID = "parsed-geojson";
const POLYGON_LAYER_ID = `${GEOJSON_SOURCE_ID}-polygon`;
const POLYGON_OUTLINE_LAYER_ID = `${GEOJSON_SOURCE_ID}-polygon-outline`;
const POLYGON_LABEL_LAYER_ID = `${GEOJSON_SOURCE_ID}-polygon-label`;
const BASE_STYLE_URL = "https://tiles.kmproj.com/styles/osm-ja-light.json";

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

let nextWorkerRequestId = 0;
let mapInstance: maplibregl.Map | undefined;

const parseXmlWithWorker = (fileName: string, xml: string) =>
  new Promise<WorkerSuccessMessage>((resolve, reject) => {
    const requestId = nextWorkerRequestId++;

    const cleanup = () => {
      parserWorker.removeEventListener("message", handleMessage);
      parserWorker.removeEventListener("error", handleError);
    };

    const handleMessage = (event: MessageEvent<WorkerMessage>) => {
      if (event.data.id !== requestId) {
        return;
      }

      cleanup();
      if (event.data.status === "success") {
        resolve(event.data);
      } else {
        reject(new Error(event.data.message));
      }
    };

    const handleError = (event: ErrorEvent) => {
      cleanup();
      reject(event.error ?? new Error(event.message));
    };

    parserWorker.addEventListener("message", handleMessage);
    parserWorker.addEventListener("error", handleError);

    parserWorker.postMessage({
      id: requestId,
      fileName,
      xml,
    });
  });

async function main() {
  const dropArea = document.getElementById("drop-area");
  const statusEl = document.getElementById("status");
  const fileInput = document.getElementById("file-input") as HTMLInputElement | null;
  initMap();

  if (!dropArea || !statusEl || !fileInput) {
    console.error("Required UI elements are missing from the page.");
    return;
  }

  const setStatus = (message: string) => {
    statusEl.textContent = message;
  };

  const handleFile = async (file: File) => {
    setStatus(`Reading ${file.name}...`);
    try {
      const text = await file.text();
      const { result, elapsedMs } = await parseXmlWithWorker(file.name, text);
      const summary = `Parsed in ${elapsedMs.toFixed(2)} ms • ${result.features.length} features`;
      setStatus(`${summary}`);
      updateMapWithGeoJson(result);
    } catch (error) {
      console.error("Failed to parse XML:", error);
      const message =
        error instanceof Error ? error.message : JSON.stringify(error);
      setStatus(`Error parsing ${file.name}: ${message}`);
    }
  };

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
      setStatus("No file detected. Please drop an XML file.");
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

  fileInput.addEventListener("change", () => {
    const file = fileInput.files?.[0];
    if (file) {
      void handleFile(file);
      fileInput.value = "";
    }
  });

  setStatus("Drop an XML file above to parse it.");
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
