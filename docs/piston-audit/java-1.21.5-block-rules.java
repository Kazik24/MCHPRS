import java.lang.reflect.*;
public class AuditBlockRules {
 public static void main(String[] args) throws Exception {
  Class.forName("ac").getDeclaredMethod("a").invoke(null);
  Class.forName("alt").getDeclaredMethod("a").invoke(null);
  Class<?> blocks=Class.forName("dnq"), block=Class.forName("dno"), state=Class.forName("ebq");
  String[][] inputs=new String[][] {{"WHITE_GLAZED_TERRACOTTA","lM"},{"STONE","b"},{"BEDROCK","I"},{"WATER","J"},{"PISTON_HEAD","bJ"},{"OBSIDIAN","cy"},{"TORCH","cz"},{"CHEST","cG"},{"REDSTONE_WIRE","cH"},{"OAK_SIGN","cP"},{"REPEATER","ey"},{"COMPARATOR","hz"},{"SLIME_BLOCK","ix"},{"IRON_TRAPDOOR","iA"},{"OBSERVER","lu"},{"BARREL","oA"},{"HONEY_BLOCK","pO"},{"CRYING_OBSIDIAN","pS"},{"RESPAWN_ANCHOR","pT"}};
  for(String[] p: inputs){ Object b=blocks.getField(p[1]).get(null); Object s=block.getMethod("m").invoke(b);
   System.out.println("RULE "+p[0]+" reaction="+state.getMethod("r").invoke(s)+" entity="+state.getMethod("x").invoke(s));
  }
 }
}