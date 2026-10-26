package dev.akaduy.wimesh;

import android.content.Context;
import android.net.ConnectivityManager;
import android.net.Network;
import android.net.NetworkCapabilities;
import android.net.wifi.WifiManager;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;

import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.security.KeyStore;
import java.util.Date;
import java.util.Set;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;

/** Runs the bundled CLI once: `--auto --once` over the current Wi-Fi network. */
final class LoginJob {
    private static final String KEY_ALIAS = "wimesh";
    private static final int LOG_LIMIT = 8 * 1024;
    // Same names as the CLI's default --ssid list, quoted like WifiInfo.getSSID()
    private static final Set<String> WI_MESH_SSIDS = Set.of("\"Wi-MESH 2.4G\"", "\"Wi-MESH 5G\"");

    static File stateDir(Context context) {
        return new File(context.getNoBackupFilesDir(), "state");
    }

    static File logFile(Context context) {
        return new File(context.getNoBackupFilesDir(), "last-run.log");
    }

    private static File sealedCredentials(Context context) {
        return new File(context.getNoBackupFilesDir(), "credentials.enc");
    }

    static boolean hasCredentials(Context context) {
        return sealedCredentials(context).exists();
    }

    static synchronized void run(Context context) {
        String output;
        File plain = new File(stateDir(context), "credentials");
        try {
            Network wifi = wifiNetwork(context);
            String ssid = currentSsid(context);
            if (wifi == null) {
                output = "Not connected to Wi-Fi, skipped.";
            } else if (!WI_MESH_SSIDS.contains(ssid)) {
                output = "Wi-Fi " + ssid + " is not Wi-MESH, skipped.";
            } else {
                stateDir(context).mkdirs();
                // The CLI reads plaintext; it only exists while the CLI runs
                write(plain, open(context));
                ProcessBuilder builder = new ProcessBuilder(
                        context.getApplicationInfo().nativeLibraryDir + "/libwimesh.so",
                        "--auto", "--once", "--no-logs",
                        "--current-ssid", ssid,
                        "--state-dir", stateDir(context).getPath(),
                        "--network-handle", Long.toString(wifi.getNetworkHandle()))
                        .redirectErrorStream(true);
                builder.environment().put("TMPDIR", context.getCacheDir().getPath());
                Process process = builder.start();
                output = read(process.getInputStream()) + "\nexit " + process.waitFor();
            }
        } catch (Exception error) {
            output = "Error: " + error;
        } finally {
            plain.delete();
        }
        if (output.length() > LOG_LIMIT) {
            output = output.substring(output.length() - LOG_LIMIT);
        }
        try {
            write(logFile(context), new Date() + "\n" + output);
        } catch (Exception ignored) {
        }
    }

    @SuppressWarnings("deprecation")
    private static Network wifiNetwork(Context context) {
        ConnectivityManager manager = context.getSystemService(ConnectivityManager.class);
        for (Network network : manager.getAllNetworks()) {
            NetworkCapabilities capabilities = manager.getNetworkCapabilities(network);
            if (capabilities != null
                    && capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)
                    // Never let mobile data or a VPN answer the internet check
                    && !capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)
                    && !capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) {
                return network;
            }
        }
        return null;
    }

    @SuppressWarnings("deprecation")
    private static String currentSsid(Context context) {
        // Needs location permission, otherwise "<unknown ssid>" and the CLI skips
        return context.getSystemService(WifiManager.class).getConnectionInfo().getSSID();
    }

    private static SecretKey key() throws Exception {
        KeyStore store = KeyStore.getInstance("AndroidKeyStore");
        store.load(null);
        if (!store.containsAlias(KEY_ALIAS)) {
            KeyGenerator generator =
                    KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
            generator.init(new KeyGenParameterSpec.Builder(KEY_ALIAS,
                    KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                    .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                    .build());
            return generator.generateKey();
        }
        return (SecretKey) store.getKey(KEY_ALIAS, null);
    }

    /** Encrypts `phone\npassword` with the Android Keystore key. */
    static void seal(Context context, String phone, String password) throws Exception {
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.ENCRYPT_MODE, key());
        byte[] sealed = cipher.doFinal((phone + "\n" + password).getBytes(StandardCharsets.UTF_8));
        write(sealedCredentials(context), Base64.encodeToString(cipher.getIV(), Base64.NO_WRAP)
                + "\n" + Base64.encodeToString(sealed, Base64.NO_WRAP));
    }

    private static String open(Context context) throws Exception {
        String[] parts = new String(Files.readAllBytes(sealedCredentials(context).toPath()),
                StandardCharsets.UTF_8).split("\n");
        Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
        cipher.init(Cipher.DECRYPT_MODE, key(),
                new GCMParameterSpec(128, Base64.decode(parts[0], Base64.NO_WRAP)));
        return new String(cipher.doFinal(Base64.decode(parts[1], Base64.NO_WRAP)),
                StandardCharsets.UTF_8);
    }

    static void forget(Context context) {
        sealedCredentials(context).delete();
        new File(stateDir(context), "card").delete();
    }

    private static void write(File file, String contents) throws Exception {
        try (FileOutputStream stream = new FileOutputStream(file)) {
            stream.write(contents.getBytes(StandardCharsets.UTF_8));
        }
    }

    private static String read(InputStream stream) throws Exception {
        ByteArrayOutputStream buffer = new ByteArrayOutputStream();
        byte[] chunk = new byte[4096];
        for (int count; (count = stream.read(chunk)) != -1; ) {
            buffer.write(chunk, 0, count);
        }
        return buffer.toString("UTF-8");
    }
}
