package mchprs.research;
import java.lang.reflect.*;
import java.nio.file.*;
import java.util.*;
import com.google.gson.GsonBuilder;
public class ExportBlockSounds {
  public static void main(String[] args) throws Exception {
    Class.forName("ac").getMethod("a").invoke(null);
    Class.forName("alt").getMethod("a").invoke(null);
    Object registry = Class.forName("mh").getField("e").get(null);
    Method key = Class.forName("jt").getMethod("b", Object.class);
    Method state = Class.forName("dno").getMethod("m");
    Method sound = Class.forName("ebp$a").getMethod("A");
    Class<?> type = Class.forName("dvl");
    Method place = type.getMethod("e"), breaking = type.getMethod("c");
    Method volume = type.getMethod("a"), pitch = type.getMethod("b");
    Method location = Class.forName("awx").getMethod("a");
    List<Map<String,Object>> result = new ArrayList<>();
    for(Object block : (Iterable<?>)registry) {
      Object profile = sound.invoke(state.invoke(block));
      Map<String,Object> row = new LinkedHashMap<>();
      row.put("name", key.invoke(registry,block).toString().replace("minecraft:", ""));
      row.put("place", location.invoke(place.invoke(profile)).toString().replace("minecraft:", ""));
      row.put("break", location.invoke(breaking.invoke(profile)).toString().replace("minecraft:", ""));
      row.put("volume", volume.invoke(profile));
      row.put("pitch", pitch.invoke(profile));
      result.add(row);
    }
    Files.writeString(Path.of(args[0]),new GsonBuilder().setPrettyPrinting().create().toJson(result)+"\n");
    System.err.println("Exported "+result.size()+" vanilla block sound profiles");
  }
}