export function downloadBlob(blob: Blob, filename: string) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement('a');
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}

export function downloadBase64(value: { data_base64: string; mime: string; filename: string }) {
  const bytes = Uint8Array.from(atob(value.data_base64), (character) => character.charCodeAt(0));
  downloadBlob(new Blob([bytes], { type: value.mime }), value.filename);
}
