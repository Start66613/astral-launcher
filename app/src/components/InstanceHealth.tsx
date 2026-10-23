import type { Instance } from "../lib/types";
import Icon from "./Icon";

export default function InstanceHealth({ instance }: { instance: Instance }) {
  const health = instance.health;
  if (health?.ok === true) return <p className="instance-health healthy"><Icon name="check" />基础文件检查通过</p>;
  const reasons = health?.reasons?.length ? health.reasons : ["基础文件状态无法确认，请刷新或检查目录。"];
  return <details className="instance-health incomplete"><summary><span>{reasons[0]}</span><b>查看原因</b></summary><ul>{reasons.map((reason, index) => <li key={index}>{reason}</li>)}</ul></details>;
}
