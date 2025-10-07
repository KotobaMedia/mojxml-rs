type WorkerSuccessMessage = {
  id: number;
  status: "success";
  result: string;
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

let nextWorkerRequestId = 0;

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
      const summary = `Parsed in ${elapsedMs.toFixed(2)} ms`;
      setStatus(`${summary}\n${result}`);
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

  ["dragenter", "dragover", "dragleave", "drop"].forEach((eventName) => {
    dropArea.addEventListener(eventName, preventDefaults);
  });

  ["dragenter", "dragover"].forEach((eventName) => {
    dropArea.addEventListener(eventName, () => dropArea.classList.add("highlight"));
  });

  ["dragleave", "drop"].forEach((eventName) => {
    dropArea.addEventListener(eventName, () => dropArea.classList.remove("highlight"));
  });

  dropArea.addEventListener("drop", (event) => {
    const dragEvent = event as DragEvent;
    const file = dragEvent.dataTransfer?.files?.[0];
    if (!file) {
      setStatus("No file detected. Please drop an XML file.");
      return;
    }
    void handleFile(file);
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
