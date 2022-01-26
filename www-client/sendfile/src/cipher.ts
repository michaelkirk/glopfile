import { base64ToArrayBuffer } from "./base64";

export class CipherKey {
  cryptoKey: CryptoKey;
  constructor(cryptoKey: CryptoKey) {
    this.cryptoKey = cryptoKey;
  }

  static async random(): Promise<CipherKey> {
    const cryptoKey: CryptoKey = await crypto.subtle.generateKey(
      { name: "AES-GCM", length: 256 },
      true,
      ["encrypt"]
    );
    return new CipherKey(cryptoKey);
  }

  async serialized(): Promise<Uint8Array> {
    const exported = await window.crypto.subtle.exportKey(
      "raw",
      this.cryptoKey
    );
    return new Uint8Array(exported);
  }

  static async fromSerializedText(serializedText: string): Promise<CipherKey> {
    let buffer = base64ToArrayBuffer(serializedText);
    const cryptoKey = await window.crypto.subtle.importKey(
      "raw",
      buffer,
      { name: "AES-GCM", length: 256 },
      true,
      ["decrypt"]
    );
    return new CipherKey(cryptoKey);
  }
}

export class ContentCipher {
  cipherKey: CipherKey;

  constructor(cipherKey: CipherKey) {
    this.cipherKey = cipherKey;
  }

  // serializes to json and outputs bytes encrypted with key
  //
  // returns: [ iv, ciphertext ]
  async encrypt(plaintext: ArrayBuffer): Promise<ArrayBuffer> {
    const iv = window.crypto.getRandomValues(new Uint8Array(12));
    const ciphertext = await crypto.subtle.encrypt(
      { name: "AES-GCM", iv },
      this.cipherKey.cryptoKey,
      plaintext
    );
    const ciphertextBytes = new Uint8Array(ciphertext);

    const length = iv.length + ciphertextBytes.length;
    const result = new Uint8Array(length);

    result.set(iv, 0);
    result.set(ciphertextBytes, iv.length);

    return result;
  }

  async decrypt(ivAndCiphertext: ArrayBuffer): Promise<ArrayBuffer> {
    let iv = new Uint8Array(ivAndCiphertext.slice(0, 12));
    let ciphertext = new Uint8Array(ivAndCiphertext.slice(12));
    const plaintext = await crypto.subtle.decrypt(
      { name: "AES-GCM", iv },
      this.cipherKey.cryptoKey,
      ciphertext
    );
    return plaintext;
  }
}
