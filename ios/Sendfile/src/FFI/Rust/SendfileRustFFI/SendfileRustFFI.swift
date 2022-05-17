import Foundation

extension SendfileError {
    var message: String {
        switch self {
        case .ClientHttpErrorResponse(let message),
                .ClientApiErrorResponse(let message),
                .Decrypt(let message),
                .HttpClient(let message),
                .InvalidCipherKey(let message),
                .InvalidInput(let message),
                .InvalidPeerMessage(let message),
                .InvalidServerResponse(let message),
                .Io(let message),
                .RtcDataChannel(let message),
                .Timeout(let message),
                .WebSocketClient(let message),
                .WebSocketClosed(let message):
            return message
        }
    }
}
