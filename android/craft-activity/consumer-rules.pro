# The Rust library calls CraftActivity.bridge and the native methods by name, and android-activity's
# C glue calls GameActivity's Java side: keep both.
-keep class io.github.craftandroid.** { *; }
-keep class com.google.androidgamesdk.** { *; }
