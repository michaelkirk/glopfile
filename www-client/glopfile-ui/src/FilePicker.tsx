import React from "react";

class FilePickerProps {
  onFileAdded: (file: File) => void;
  constructor(onFileAdded: (file: File) => void) {
    this.onFileAdded = onFileAdded;
  }
}

class FilePicker extends React.Component<FilePickerProps, {}> {
  render(): React.ReactElement {
    return (
      <div className="FilePicker">
        <div
          className="dropZone"
          onDrop={this.onDropHandler.bind(this)}
          onDragOver={this.onDragOverHandler.bind(this)}
        >
          <p className="dropZoneInstructions">
            Drag one or more files to this Drop Zone.
          </p>
          <p className="pickerInstructions">
            Or you can choose files to upload:
            <input type="file" onChange={this.onFileInputChange.bind(this)} />
          </p>
        </div>
      </div>
    );
  }

  addFile(file: File): void {
    this.props.onFileAdded(file);
  }

  // Events

  onDropHandler(event: React.DragEvent<HTMLDivElement>): void {
    event.preventDefault();

    if (event.dataTransfer.items) {
      // Use DataTransferItemList interface to access the file(s)
      for (let i = 0; i < event.dataTransfer.items.length; i++) {
        const item = event.dataTransfer.items[i];
        // If dropped items aren't files, reject them
        if (item.kind !== "file") {
          console.warn("found non-file item", item);
          continue;
        }

        const file = item.getAsFile()!;
        this.addFile(file);
      }
    } else {
      // Use DataTransfer interface to access the file(s)
      for (var i = 0; i < event.dataTransfer.files.length; i++) {
        const file = event.dataTransfer.files[i];
        this.addFile(file);
      }
    }
  }

  onDragOverHandler(event: React.DragEvent<HTMLDivElement>): void {
    // Without this `preventDefault`, rather than send info about the file for upload, the browser will open the file in the browser
    event.preventDefault();
  }

  onFileInputChange(event: React.ChangeEvent<HTMLInputElement>): void {
    const files = event.target.files;
    if (files == null) {
      console.error("event was missing files property");
      return;
    }
    if (files.length === 0) {
      console.error("files was empty");
      return;
    }
    for (let i = 0; i < files.length; i++) {
      const file = files[i];
      this.addFile(file);
    }
  }
}

export default FilePicker;
