package pl.redstonefun.chat;

import java.io.*;
import java.nio.ByteBuffer;
import java.nio.charset.CharacterCodingException;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import java.util.UUID;

/** Versioned frames; identity is deliberately absent and comes from the connection. */
public final class ChatProtocol {
    public static final String CHANNEL = "redstonefun:chat";
    public static final int VERSION = 1, HELLO = 0, READY = 1, CHAT = 2;
    public static final int MAX_FRAME = 1052;

    public record Frame(int kind, UUID token, long sequence, String text) {}

    private ChatProtocol() {}

    public static boolean validText(String text) {
        return !text.isBlank() && text.length() <= 256
            && text.codePoints().noneMatch(c -> Character.isISOControl(c) || c == 0x00a7)
            && text.getBytes(StandardCharsets.UTF_8).length <= 1024;
    }

    public static byte[] handshake(int kind, UUID token) {
        if (kind != HELLO && kind != READY) throw new IllegalArgumentException("Invalid handshake");
        return ByteBuffer.allocate(18).put((byte) VERSION).put((byte) kind)
            .putLong(token.getMostSignificantBits()).putLong(token.getLeastSignificantBits()).array();
    }

    public static byte[] chat(UUID token, long sequence, String text) {
        if (sequence <= 0 || !validText(text)) throw new IllegalArgumentException("Invalid chat");
        byte[] utf8 = text.getBytes(StandardCharsets.UTF_8);
        return ByteBuffer.allocate(28 + utf8.length).put((byte) VERSION).put((byte) CHAT)
            .putLong(token.getMostSignificantBits()).putLong(token.getLeastSignificantBits())
            .putLong(sequence).putShort((short) utf8.length).put(utf8).array();
    }

    public static Frame decode(byte[] bytes) throws IOException {
        if (bytes.length < 18 || bytes.length > MAX_FRAME) throw new IOException("Invalid frame size");
        var in = new DataInputStream(new ByteArrayInputStream(bytes));
        if (in.readUnsignedByte() != VERSION) throw new IOException("Unsupported bridge version");
        int kind = in.readUnsignedByte();
        UUID token = new UUID(in.readLong(), in.readLong());
        if (kind == HELLO || kind == READY) {
            if (in.available() != 0) throw new IOException("Trailing handshake data");
            return new Frame(kind, token, 0, "");
        }
        if (kind != CHAT) throw new IOException("Unknown bridge operation");
        long sequence = in.readLong();
        int length = in.readUnsignedShort();
        if (sequence <= 0 || length > 1024 || length != in.available()) throw new IOException("Invalid chat frame");
        try {
            String text = StandardCharsets.UTF_8.newDecoder()
                .onMalformedInput(CodingErrorAction.REPORT).onUnmappableCharacter(CodingErrorAction.REPORT)
                .decode(ByteBuffer.wrap(in.readNBytes(length))).toString();
            if (!validText(text)) throw new IOException("Invalid chat text");
            return new Frame(kind, token, sequence, text);
        } catch (CharacterCodingException ex) {
            throw new IOException("Invalid UTF-8", ex);
        }
    }
}
