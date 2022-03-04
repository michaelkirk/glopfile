class Base64 {
  // conventional base64 encoding
  static bodyEncode(buffer: ArrayBuffer): string {
    let binary = "";
    const bytes = new Uint8Array(buffer);
    for (var i = 0; i < bytes.byteLength; i++) {
      binary += String.fromCharCode(bytes[i]);
    }
    return window.btoa(binary);
  }

  // conventional base64 decoding
  static bodyDecode(base64: string): ArrayBuffer {
    var binary_string = window.atob(base64);
    var len = binary_string.length;
    var bytes = new Uint8Array(len);
    for (var i = 0; i < len; i++) {
      bytes[i] = binary_string.charCodeAt(i);
    }
    return bytes.buffer;
  }

  // base64 encoding with a url safe alphabet
  static urlSafeEncode(buffer: ArrayBuffer): string {
    const base64 = Base64.bodyEncode(buffer);

    // replace the non-url safe chars
    return base64.replace(/\+/g, "-").replace(/\//g, "_").replace(/=/g, "~");
  }

  // base64 deeoding from a url safe alphabet
  static urlSafeDecode(urlSafeBase64: string): ArrayBuffer {
    // reverse the url safe characters back to conventional base64
    let base64 = urlSafeBase64
      .replace(/-/g, "+")
      .replace(/_/g, "/")
      .replace(/~/g, "=");

    return Base64.bodyDecode(base64);
  }
}

export default Base64;
