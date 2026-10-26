package dev.akaduy.wimesh;

import android.Manifest;
import android.app.Activity;
import android.content.pm.PackageManager;
import android.os.Build;
import android.os.Bundle;
import android.text.InputType;
import android.view.Window;
import android.view.WindowInsets;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;

import java.nio.charset.StandardCharsets;
import java.nio.file.Files;

/** One screen: save the shop account once, then everything runs on Wi-Fi connect. */
public class MainActivity extends Activity {
    private TextView status;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        // Android 15 draws apps edge-to-edge, so the old title bar would cover the form
        requestWindowFeature(Window.FEATURE_NO_TITLE);
        TextView title = new TextView(this);
        title.setText("Wi-MESH Login");
        title.setTextSize(24);
        EditText phone = new EditText(this);
        phone.setHint("Shop phone number");
        phone.setInputType(InputType.TYPE_CLASS_PHONE);
        EditText password = new EditText(this);
        password.setHint("Shop password");
        password.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
        Button save = new Button(this);
        save.setText("Save and enable");
        Button runNow = new Button(this);
        runNow.setText("Run now");
        Button forget = new Button(this);
        forget.setText("Forget account");
        status = new TextView(this);
        status.setTextIsSelectable(true);

        save.setOnClickListener(view -> {
            String number = phone.getText().toString().trim();
            String secret = password.getText().toString();
            if (number.isEmpty() || secret.isEmpty()) {
                status.setText("Enter the shop phone number and password.");
                return;
            }
            try {
                LoginJob.seal(this, number, secret);
                NetworkReceiver.register(this);
                password.setText("");
                status.setText("Saved. Logs in automatically when Wi-MESH connects.");
                requestLocation();
            } catch (Exception error) {
                status.setText("Could not save: " + error);
            }
        });
        runNow.setOnClickListener(view -> {
            status.setText("Running...");
            new Thread(() -> {
                LoginJob.run(this);
                runOnUiThread(this::showLog);
            }).start();
        });
        forget.setOnClickListener(view -> {
            LoginJob.forget(this);
            status.setText("Account forgotten. Auto login is off.");
        });

        LinearLayout layout = new LinearLayout(this);
        layout.setOrientation(LinearLayout.VERTICAL);
        int padding = (int) (16 * getResources().getDisplayMetrics().density);
        layout.addView(title);
        layout.addView(phone);
        layout.addView(password);
        layout.addView(save);
        layout.addView(runNow);
        layout.addView(forget);
        layout.addView(status);
        ScrollView root = new ScrollView(this);
        root.addView(layout);
        root.setOnApplyWindowInsetsListener((view, insets) -> {
            applyInsets(layout, padding, insets);
            return insets;
        });
        setContentView(root);
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (LoginJob.hasCredentials(this)) {
            // Also re-arms the callback if the app was force-stopped
            NetworkReceiver.register(this);
            showLog();
        } else {
            status.setText("Save your shop account to enable auto login.");
        }
    }

    /** Keeps the form clear of the status bar, navigation bar and keyboard. */
    @SuppressWarnings("deprecation")
    private static void applyInsets(LinearLayout layout, int padding, WindowInsets insets) {
        layout.setPadding(padding + insets.getSystemWindowInsetLeft(),
                padding + insets.getSystemWindowInsetTop(),
                padding + insets.getSystemWindowInsetRight(),
                padding + insets.getSystemWindowInsetBottom());
    }

    /** Reading the Wi-Fi name needs location; background runs need "Allow all the time". */
    private void requestLocation() {
        if (checkSelfPermission(Manifest.permission.ACCESS_FINE_LOCATION)
                != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(new String[] {Manifest.permission.ACCESS_FINE_LOCATION}, 1);
        } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q
                && checkSelfPermission(Manifest.permission.ACCESS_BACKGROUND_LOCATION)
                != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(new String[] {Manifest.permission.ACCESS_BACKGROUND_LOCATION}, 2);
        }
    }

    @Override
    public void onRequestPermissionsResult(int code, String[] permissions, int[] results) {
        if (code == 1 && results.length > 0 && results[0] == PackageManager.PERMISSION_GRANTED) {
            requestLocation();
        }
    }

    private void showLog() {
        try {
            status.setText(new String(Files.readAllBytes(LoginJob.logFile(this).toPath()),
                    StandardCharsets.UTF_8));
        } catch (Exception error) {
            status.setText("Auto login is on. No runs yet.");
        }
    }
}
